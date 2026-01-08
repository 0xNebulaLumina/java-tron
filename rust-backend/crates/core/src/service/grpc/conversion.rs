// Conversion helpers between protobuf and execution types
// Functions for converting transactions, contexts, and results between Java and Rust formats

use std::collections::HashMap;
use tracing::{debug, warn};
use revm_primitives::hex;
use tron_backend_execution::{TronContractParameter, TronExecutionContext, TronExecutionResult, TronStateChange, TronTransaction, AccountAext, TouchedKey};
use crate::backend::*;
use super::super::BackendService;
use super::address::{strip_tron_address_prefix, add_tron_address_prefix};

impl BackendService {
    // Helper functions for converting between protobuf and execution types
    pub(super) fn convert_protobuf_transaction(&self, tx: Option<&crate::backend::TronTransaction>) -> Result<(TronTransaction, crate::backend::TxKind), String> {
        let tx = tx.ok_or("Transaction is required")?;

        // Log the raw transaction data from Java
        debug!("Raw transaction from Java - energy_limit: {}, energy_price: {}, data_len: {}, contract_type: {}, asset_id_len: {}",
               tx.energy_limit, tx.energy_price, tx.data.len(), tx.contract_type, tx.asset_id.len());

        // Extract tx_kind first - needed to properly handle contract_type and special-case parsing.
        let tx_kind = crate::backend::TxKind::try_from(tx.tx_kind).unwrap_or(crate::backend::TxKind::Vm);

        // Extract contract_type and asset_id from protobuf for TRON system contracts
        // NOTE: AccountCreateContract has enum value 0, which is proto3's default.
        // For NON_VM transactions, we must always try to parse contract_type since 0 is valid.
        // For VM transactions, contract_type=0 genuinely means "unset" (not AccountCreateContract).
        let contract_type = if tx_kind == crate::backend::TxKind::NonVm || tx.contract_type != 0 {
            match tron_backend_execution::TronContractType::try_from(tx.contract_type) {
                Ok(ct) => {
                    debug!("Parsed contract type: {:?}", ct);
                    Some(ct)
                }
                Err(e) => {
                    warn!("Invalid contract type {}: {}, ignoring", tx.contract_type, e);
                    None
                }
            }
        } else {
            None
        };

        // Convert bytes to Address (strip Tron 0x41 prefix if present).
        //
        // Some conformance fixtures intentionally include malformed owner addresses, while java-tron
        // still serializes `request.pb` with the malformed bytes in `from`. Allow conversion to
        // proceed for those system contracts so contract-level validation can produce the expected
        // error messages.
        let allow_malformed_from = matches!(
            contract_type,
            Some(tron_backend_execution::TronContractType::AccountCreateContract)
                | Some(tron_backend_execution::TronContractType::AccountPermissionUpdateContract)
                | Some(tron_backend_execution::TronContractType::AccountUpdateContract)
                | Some(tron_backend_execution::TronContractType::AssetIssueContract)
                | Some(tron_backend_execution::TronContractType::UpdateAssetContract)
                | Some(tron_backend_execution::TronContractType::UpdateEnergyLimitContract)
                | Some(tron_backend_execution::TronContractType::UpdateSettingContract)
                | Some(tron_backend_execution::TronContractType::UpdateBrokerageContract)
                | Some(tron_backend_execution::TronContractType::SetAccountIdContract)
                | Some(tron_backend_execution::TronContractType::ClearAbiContract)
                | Some(tron_backend_execution::TronContractType::CancelAllUnfreezeV2Contract)
                | Some(tron_backend_execution::TronContractType::TransferAssetContract)
                | Some(tron_backend_execution::TronContractType::TransferContract)
        );
        let from = match strip_tron_address_prefix(&tx.from) {
            Ok(from_bytes) => revm_primitives::Address::from_slice(from_bytes),
            Err(e) if allow_malformed_from => {
                debug!("Allowing malformed from address for {:?}: {}", contract_type, e);
                revm_primitives::Address::ZERO
            }
            Err(e) => return Err(e),
        };

        // Phase 0.5: Fix CreateSmartContract toAddress semantics
        // When tx_kind=VM and contract_type=CREATE_SMART_CONTRACT (30), Java sends a 20-byte
        // zero array for toAddress. Rust must treat this as None (contract creation), not
        // Some(Address::ZERO) which would be interpreted as a call to address 0.
        let to = if tx.to.is_empty() {
            None // Contract creation (empty to field)
        } else {
            let to_bytes = strip_tron_address_prefix(&tx.to)?;
            let to_address = revm_primitives::Address::from_slice(to_bytes);

            // For VM contract creation, treat all-zero address as None
            // This is needed because Java sends new byte[20] (all zeros) for CreateSmartContract
            let is_vm_create =
                tx_kind == crate::backend::TxKind::Vm && tx.contract_type == 30; // CREATE_SMART_CONTRACT = 30

            if is_vm_create && to_address == revm_primitives::Address::ZERO {
                debug!("CreateSmartContract detected: treating zero address as contract creation (to=None)");
                None
            } else {
                Some(to_address)
            }
        };

        // Convert bytes to U256 (32 bytes max)
        let value = if tx.value.len() <= 32 {
            revm_primitives::U256::from_be_slice(&tx.value)
        } else {
            return Err("Invalid value length".to_string());
        };

        // Get execution config to check coinbase compatibility
        let execution_config = self.get_execution_config()?;

        // TRON Parity Fix: Force gas_price = 0 unless coinbase compat is explicitly enabled
        let gas_price = if execution_config.evm_eth_coinbase_compat {
            // Legacy behavior: convert energy_price to gas_price
            let energy_price_sun = tx.energy_price as u64;
            if energy_price_sun == 0 {
                revm_primitives::U256::from(1u64) // Minimum gas price
            } else {
                // Convert SUN to a reasonable gas price range (1-1000)
                let converted_price = std::cmp::min(energy_price_sun / 1000, 1000);
                let final_price = std::cmp::max(converted_price, 1); // Minimum 1
                revm_primitives::U256::from(final_price)
            }
        } else {
            // TRON mode: Always use gas_price = 0 to prevent coinbase/miner rewards
            debug!("Using gas_price = 0 for TRON parity (suppresses coinbase rewards)");
            revm_primitives::U256::ZERO
        };

        debug!("Gas price conversion - original energy_price: {} SUN, final gas_price: {}, coinbase_compat: {}",
               tx.energy_price, gas_price, execution_config.evm_eth_coinbase_compat);

        // Handle zero energy_limit by using a reasonable default
        let gas_limit = if tx.energy_limit == 0 {
            // Use a default gas limit based on transaction type and data size
            let base_gas = 21000u64; // Basic transaction cost
            let data_gas = tx.data.len() as u64 * 16; // 16 gas per byte of data
            let default_limit = base_gas + data_gas + 100000; // Add buffer for contract execution
            debug!("Using default gas limit {} for zero energy_limit transaction", default_limit);
            default_limit
        } else {
            tx.energy_limit as u64
        };

        let asset_id = if !tx.asset_id.is_empty() {
            debug!("Parsed asset_id: {} bytes", tx.asset_id.len());
            Some(tx.asset_id.clone())
        } else {
            None
        };

        let contract_parameter = tx.contract_parameter.as_ref().map(|any| TronContractParameter {
            type_url: any.type_url.clone(),
            value: any.value.clone(),
        });

        let metadata = tron_backend_execution::TxMetadata {
            contract_type,
            asset_id,
            from_raw: Some(tx.from.clone()),
            contract_parameter,
        };

        let transaction = TronTransaction {
            from,
            to,
            value,
            data: revm_primitives::Bytes::from(tx.data.clone()),
            gas_limit,
            gas_price,
            nonce: tx.nonce as u64,
            metadata,
        };

        debug!("Transaction kind: {:?}, contract_type: {:?}", tx_kind, transaction.metadata.contract_type);

        Ok((transaction, tx_kind))
    }

    pub(super) fn convert_protobuf_context(&self, ctx: Option<&crate::backend::ExecutionContext>) -> Result<TronExecutionContext, String> {
        let ctx = ctx.ok_or("Execution context is required")?;

        // Log the raw context data from Java
        debug!("Raw context from Java - energy_limit: {}, energy_price: {}",
               ctx.energy_limit, ctx.energy_price);

        // Strip Tron 0x41 prefix from coinbase address if present
        let coinbase_bytes = strip_tron_address_prefix(&ctx.coinbase)?;
        let block_coinbase = revm_primitives::Address::from_slice(coinbase_bytes);

        // Handle zero energy_limit by using the configured block gas limit
        let block_gas_limit = if ctx.energy_limit == 0 {
            // Use the configured energy_limit from config as the block gas limit
            1000000000u64 // 1B gas limit (same as config.toml)
        } else {
            ctx.energy_limit as u64
        };

        debug!("Using block_gas_limit: {}", block_gas_limit);

        let transaction_id = if ctx.transaction_id.is_empty() {
            None
        } else {
            let trimmed = ctx.transaction_id.trim_start_matches("0x");
            match hex::decode(trimmed) {
                Ok(bytes) if bytes.len() == 32 => Some(revm_primitives::B256::from_slice(&bytes)),
                Ok(bytes) => {
                    warn!(
                        "Invalid transaction_id length: expected 32 bytes, got {}",
                        bytes.len()
                    );
                    None
                }
                Err(e) => {
                    warn!("Failed to decode transaction_id hex: {}", e);
                    None
                }
            }
        };

        Ok(TronExecutionContext {
            block_number: ctx.block_number as u64,
            block_timestamp: ctx.block_timestamp as u64,
            block_coinbase,
            block_difficulty: revm_primitives::U256::from(1u64), // Default difficulty
            block_gas_limit,
            chain_id: 0x2b6653dc, // Tron mainnet chain ID
            energy_price: ctx.energy_price as u64,
            bandwidth_price: 1000, // Default bandwidth price
            transaction_id,
        })
    }

    pub(super) fn convert_call_contract_request_to_transaction(&self, req: &crate::backend::CallContractRequest) -> Result<TronTransaction, String> {
        // Convert bytes to Address (strip Tron 0x41 prefix if present)
        let from_bytes = strip_tron_address_prefix(&req.from)?;
        let from = revm_primitives::Address::from_slice(from_bytes);

        let to_bytes = strip_tron_address_prefix(&req.to)?;
        let to = revm_primitives::Address::from_slice(to_bytes);

        Ok(TronTransaction {
            from,
            to: Some(to),
            value: revm_primitives::U256::ZERO, // Contract calls typically don't transfer value
            data: req.data.clone().into(),
            gas_limit: 1000000, // Default gas limit for contract calls
            gas_price: revm_primitives::U256::from(1),
            nonce: 0, // Contract calls don't use nonce
            metadata: tron_backend_execution::TxMetadata::default(), // No specific metadata for contract calls
        })
    }

    /// Convert a TronExecutionResult to protobuf ExecuteTransactionResponse.
    ///
    /// # Arguments
    /// * `result` - The execution result from Rust backend
    /// * `pre_exec_aext` - Pre-execution AEXT values for hybrid mode
    /// * `touched_keys` - Optional list of database keys touched during execution (for B-镜像)
    /// * `write_mode` - The write mode: 0 = COMPUTE_ONLY, 1 = PERSISTED
    pub(super) fn convert_execution_result_to_protobuf(
        &self,
        result: TronExecutionResult,
        pre_exec_aext: &std::collections::HashMap<revm::primitives::Address, AccountAext>,
        touched_keys: Option<&[TouchedKey]>,
        write_mode: i32,
    ) -> ExecuteTransactionResponse {
        let status = if result.success {
            execution_result::Status::Success
        } else {
            execution_result::Status::Revert
        };

        let logs: Vec<LogEntry> = result.logs.iter().map(|log| {
            LogEntry {
                address: add_tron_address_prefix(&log.address),
                topics: log.topics().iter().map(|t| t.as_slice().to_vec()).collect(),
                data: log.data.data.to_vec(),
            }
        }).collect();

        let state_changes: Vec<StateChange> = result.state_changes.iter().map(|change| {
            match change {
                TronStateChange::StorageChange { address, key, old_value, new_value } => {
                    StateChange {
                        change: Some(crate::backend::state_change::Change::StorageChange(
                            crate::backend::StorageChange {
                                address: add_tron_address_prefix(address),
                                key: key.to_be_bytes::<32>().to_vec(),
                                old_value: old_value.to_be_bytes::<32>().to_vec(),
                                new_value: new_value.to_be_bytes::<32>().to_vec(),
                            }
                        ))
                    }
                },
                TronStateChange::AccountChange { address, old_account, new_account } => {
                    // Get AEXT mode from config
                    let aext_mode = self.get_execution_config()
                        .ok()
                        .and_then(|cfg| Some(cfg.remote.accountinfo_aext_mode.as_str()))
                        .unwrap_or("none");

                    // Helper function to convert AccountInfo to protobuf
                    // is_old: true for old_account, false for new_account
                    let convert_account_info = |addr: &revm::primitives::Address, acc_info: &revm::primitives::AccountInfo, is_old: bool| {
                        // Ensure EOAs (no code) serialize with empty code bytes.
                        let code_bytes: Vec<u8> = match acc_info.code.as_ref() {
                            None => Vec::new(),
                            Some(c) => {
                                let b = c.bytes();
                                // Treat zero-length or single 0x00 as empty for EOAs
                                if b.len() == 0 || (b.len() == 1 && b[0] == 0) {
                                    Vec::new()
                                } else {
                                    b.to_vec()
                                }
                            }
                        };

                        // Canonical empty code hash: keccak256("")
                        const KECCAK_EMPTY: [u8; 32] = [
                            0xc5, 0xd2, 0x46, 0x01, 0x86, 0xf7, 0x23, 0x3c,
                            0x92, 0x7e, 0x7d, 0xb2, 0xdc, 0xc7, 0x03, 0xc0,
                            0xe5, 0x00, 0xb6, 0x53, 0xca, 0x82, 0x27, 0x3b,
                            0x7b, 0xfa, 0xd8, 0x04, 0x5d, 0x85, 0xa4, 0x70,
                        ];

                        // Normalize code_hash for empty code to KECCAK_EMPTY for parity with embedded
                        let code_hash_bytes: Vec<u8> = if code_bytes.is_empty() {
                            KECCAK_EMPTY.to_vec()
                        } else {
                            acc_info.code_hash.as_slice().to_vec()
                        };

                        // Determine if this is an EOA (empty code)
                        let is_eoa = code_bytes.is_empty();

                        // Populate AEXT fields based on mode
                        let (net_usage, free_net_usage, energy_usage, latest_consume_time,
                             latest_consume_free_time, latest_consume_time_for_energy,
                             net_window_size, net_window_optimized, energy_window_size,
                             energy_window_optimized) = match aext_mode {
                            "hybrid" if is_eoa => {
                                // Hybrid mode: prefer pre-provided AEXT from Java, fallback to defaults
                                if let Some(aext) = pre_exec_aext.get(addr) {
                                    debug!("Using pre-exec AEXT for address {} in hybrid mode", hex::encode(add_tron_address_prefix(addr)));
                                    // Use the same AEXT for both old and new (unchanged fields)
                                    (Some(aext.net_usage), Some(aext.free_net_usage), Some(aext.energy_usage),
                                     Some(aext.latest_consume_time), Some(aext.latest_consume_free_time),
                                     Some(aext.latest_consume_time_for_energy), Some(aext.net_window_size),
                                     Some(aext.net_window_optimized), Some(aext.energy_window_size),
                                     Some(aext.energy_window_optimized))
                                } else {
                                    // Not provided, fall back to defaults
                                    debug!("No pre-exec AEXT for address {}, using defaults in hybrid mode", hex::encode(add_tron_address_prefix(addr)));
                                    (Some(0), Some(0), Some(0), Some(0), Some(0), Some(0),
                                     Some(28800), Some(false), Some(28800), Some(false))
                                }
                            },
                            "zeros" if is_eoa => {
                                // For EOAs in "zeros" mode, populate all fields with zero/false
                                (Some(0), Some(0), Some(0), Some(0), Some(0), Some(0),
                                 Some(0), Some(false), Some(0), Some(false))
                            },
                            "defaults" if is_eoa => {
                                // For EOAs in "defaults" mode, match embedded Java-Tron defaults:
                                // - net_window_size = 28800 (0x7080)
                                // - energy_window_size = 28800 (0x7080)
                                // - All other fields zero/false
                                // This ensures byte-level AEXT tail parity with embedded CSVs
                                (Some(0), Some(0), Some(0), Some(0), Some(0), Some(0),
                                 Some(28800), Some(false), Some(28800), Some(false))
                            },
                            "tracked" if is_eoa => {
                                // Populate with real values from resource tracking
                                // Look up address in aext_map
                                if let Some((before_aext, after_aext)) = result.aext_map.get(addr) {
                                    // Use before_aext for old_account, after_aext for new_account
                                    let aext = if is_old { before_aext } else { after_aext };

                                    (Some(aext.net_usage), Some(aext.free_net_usage), Some(aext.energy_usage),
                                     Some(aext.latest_consume_time), Some(aext.latest_consume_free_time),
                                     Some(aext.latest_consume_time_for_energy), Some(aext.net_window_size),
                                     Some(aext.net_window_optimized), Some(aext.energy_window_size),
                                     Some(aext.energy_window_optimized))
                                } else {
                                    // Address not in aext_map, use defaults
                                    (Some(0), Some(0), Some(0), Some(0), Some(0), Some(0),
                                     Some(28800), Some(false), Some(28800), Some(false))
                                }
                            },
                            "tracked" => {
                                // Non-EOA contracts in tracked mode: no AEXT
                                (None, None, None, None, None, None, None, None, None, None)
                            },
                            "hybrid" => {
                                // Non-EOA contracts in hybrid mode: no AEXT
                                (None, None, None, None, None, None, None, None, None, None)
                            },
                            _ => {
                                // "none" or unknown: leave all fields as None (current behavior)
                                (None, None, None, None, None, None, None, None, None, None)
                            }
                        };

                        debug!("AccountInfo AEXT presence: mode={}, is_eoa={}, address={}, net_window={:?}, energy_window={:?}",
                               aext_mode, is_eoa, hex::encode(add_tron_address_prefix(addr)),
                               net_window_size, energy_window_size);

                        crate::backend::AccountInfo {
                            address: add_tron_address_prefix(addr),
                            balance: acc_info.balance.to_be_bytes::<32>().to_vec(),
                            nonce: acc_info.nonce,
                            code_hash: code_hash_bytes,
                            code: code_bytes,
                            // Optional resource usage fields (AEXT) - populated based on mode
                            net_usage,
                            free_net_usage,
                            energy_usage,
                            latest_consume_time,
                            latest_consume_free_time,
                            latest_consume_time_for_energy,
                            net_window_size,
                            net_window_optimized,
                            energy_window_size,
                            energy_window_optimized,
                        }
                    };

                    let old_account_proto = old_account.as_ref().map(|acc| convert_account_info(address, acc, true));
                    let new_account_proto = new_account.as_ref().map(|acc| convert_account_info(address, acc, false));

                    StateChange {
                        change: Some(crate::backend::state_change::Change::AccountChange(
                            crate::backend::AccountChange {
                                address: add_tron_address_prefix(address),
                                old_account: old_account_proto,
                                new_account: new_account_proto,
                                is_creation: old_account.is_none() && new_account.is_some(),
                                is_deletion: old_account.is_some() && new_account.is_none(),
                            }
                        ))
                    }
                }
            }
        }).collect();

        // Convert freeze changes from execution result to protobuf
        let freeze_changes: Vec<crate::backend::FreezeLedgerChange> = result.freeze_changes.iter().map(|change| {
            use crate::backend::freeze_ledger_change::Resource;
            use tron_backend_execution::FreezeLedgerResource;

            let resource = match change.resource {
                FreezeLedgerResource::Bandwidth => Resource::Bandwidth,
                FreezeLedgerResource::Energy => Resource::Energy,
                FreezeLedgerResource::TronPower => Resource::TronPower,
            };

            crate::backend::FreezeLedgerChange {
                owner_address: add_tron_address_prefix(&change.owner_address),
                resource: resource as i32,
                amount: change.amount,
                expiration_ms: change.expiration_ms,
                v2_model: change.v2_model,
            }
        }).collect();

        // Convert global resource totals changes from execution result to protobuf
        let global_resource_changes: Vec<crate::backend::GlobalResourceTotalsChange> = result.global_resource_changes.iter().map(|change| {
            crate::backend::GlobalResourceTotalsChange {
                total_net_weight: change.total_net_weight,
                total_net_limit: change.total_net_limit,
                total_energy_weight: change.total_energy_weight,
                total_energy_limit: change.total_energy_limit,
            }
        }).collect();

        // Convert TRC-10 changes from execution result to protobuf (Phase 2)
        let trc10_changes: Vec<crate::backend::Trc10Change> = result.trc10_changes.iter().map(|change| {
            match change {
                tron_backend_execution::Trc10Change::AssetIssued(issued) => {
                    crate::backend::Trc10Change {
                        kind: Some(crate::backend::trc10_change::Kind::AssetIssued(
                            crate::backend::Trc10AssetIssued {
                                owner_address: add_tron_address_prefix(&issued.owner_address),
                                name: issued.name.clone(),
                                abbr: issued.abbr.clone(),
                                total_supply: issued.total_supply,
                                trx_num: issued.trx_num,
                                precision: issued.precision,
                                num: issued.num,
                                start_time: issued.start_time,
                                end_time: issued.end_time,
                                description: issued.description.clone(),
                                url: issued.url.clone(),
                                free_asset_net_limit: issued.free_asset_net_limit,
                                public_free_asset_net_limit: issued.public_free_asset_net_limit,
                                public_free_asset_net_usage: issued.public_free_asset_net_usage,
                                public_latest_free_net_time: issued.public_latest_free_net_time,
                                token_id: issued.token_id.clone().unwrap_or_default(),
                            }
                        ))
                    }
                },
                tron_backend_execution::Trc10Change::AssetTransferred(transferred) => {
                    crate::backend::Trc10Change {
                        kind: Some(crate::backend::trc10_change::Kind::AssetTransferred(
                            crate::backend::Trc10AssetTransferred {
                                owner_address: add_tron_address_prefix(&transferred.owner_address),
                                to_address: add_tron_address_prefix(&transferred.to_address),
                                asset_name: transferred.asset_name.clone(),
                                token_id: transferred.token_id.clone().unwrap_or_default(),
                                amount: transferred.amount,
                            }
                        ))
                    }
                }
            }
        }).collect();

        // Convert VoteChange from execution result to protobuf (Phase 2: Account.votes update)
        let vote_changes: Vec<crate::backend::VoteChange> = result.vote_changes.iter().map(|change| {
            crate::backend::VoteChange {
                owner_address: add_tron_address_prefix(&change.owner_address),
                votes: change.votes.iter().map(|v| crate::backend::Vote {
                    vote_address: add_tron_address_prefix(&v.vote_address),
                    vote_count: v.vote_count as i64,
                }).collect(),
            }
        }).collect();

        // Convert WithdrawChange from execution result to protobuf (WithdrawBalanceContract sidecar)
        let withdraw_changes: Vec<crate::backend::WithdrawChange> = result.withdraw_changes.iter().map(|change| {
            crate::backend::WithdrawChange {
                owner_address: add_tron_address_prefix(&change.owner_address),
                amount: change.amount,
                latest_withdraw_time: change.latest_withdraw_time,
            }
        }).collect();

        let error_message = result.error.unwrap_or_default();

        // Phase 2.I L2: Convert contract_address to TRON 21-byte format if present
        let contract_address_bytes = result.contract_address.map(|addr| {
            add_tron_address_prefix(&addr)
        }).unwrap_or_default();

        ExecuteTransactionResponse {
            result: Some(ExecutionResult {
                status: status as i32,
                return_data: result.return_data.to_vec(),
                energy_used: result.energy_used as i64,
                energy_refunded: 0, // Not provided by TronExecutionResult
                state_changes,
                logs,
                error_message: error_message.clone(),
                bandwidth_used: result.bandwidth_used as i64,
                resource_usage: vec![], // Not implemented yet
                freeze_changes, // Converted from TronExecutionResult
                global_resource_changes, // Converted from TronExecutionResult
                trc10_changes, // Phase 2: Converted TRC-10 semantic changes
                vote_changes, // Phase 2: VoteChange for Account.votes update
                withdraw_changes, // WithdrawBalanceContract: allowance/latestWithdrawTime sidecar
                // Phase 0.4: Receipt passthrough - serialized Protocol.Transaction.Result bytes
                tron_transaction_result: result.tron_transaction_result.clone().unwrap_or_default(),
                // Phase 2.I L2: Contract address for CreateSmartContract receipt
                contract_address: contract_address_bytes,
            }),
            success: result.success,
            error_message,
            // Phase B: Write mode and touched keys for B-镜像 support
            write_mode,
            touched_keys: touched_keys
                .map(|keys| {
                    keys.iter()
                        .map(|tk| DbKey {
                            db: tk.db.clone(),
                            key: tk.key.clone(),
                            is_delete: tk.is_delete,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

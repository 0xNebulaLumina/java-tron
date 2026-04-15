// Conversion helpers between protobuf and execution types
// Functions for converting transactions, contexts, and results between Java and Rust formats

use super::super::BackendService;
use super::address::{add_tron_address_prefix_with, strip_tron_address_prefix};
use crate::backend::*;
use revm_primitives::hex;
use std::collections::HashMap;
use tracing::{debug, warn};
use tron_backend_execution::{
    AccountAext, TouchedKey, TronContractParameter, TronExecutionContext, TronExecutionResult,
    TronStateChange, TronTransaction,
};

impl BackendService {
    // Helper functions for converting between protobuf and execution types
    pub(super) fn convert_protobuf_transaction(
        &self,
        tx: Option<&crate::backend::TronTransaction>,
        transaction_bytes_size: i64,
    ) -> Result<(TronTransaction, crate::backend::TxKind), String> {
        let tx = tx.ok_or("Transaction is required")?;

        // Log the raw transaction data from Java
        debug!("Raw transaction from Java - energy_limit: {}, energy_price: {}, data_len: {}, contract_type: {}, asset_id_len: {}",
               tx.energy_limit, tx.energy_price, tx.data.len(), tx.contract_type, tx.asset_id.len());

        // Extract tx_kind first - needed to properly handle contract_type and special-case parsing.
        let tx_kind =
            crate::backend::TxKind::try_from(tx.tx_kind).unwrap_or(crate::backend::TxKind::Vm);

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
                    warn!(
                        "Invalid contract type {}: {}, ignoring",
                        tx.contract_type, e
                    );
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
                | Some(tron_backend_execution::TronContractType::VoteWitnessContract)
                | Some(tron_backend_execution::TronContractType::WitnessCreateContract)
                | Some(tron_backend_execution::TronContractType::WitnessUpdateContract)
                | Some(tron_backend_execution::TronContractType::WithdrawBalanceContract)
                | Some(tron_backend_execution::TronContractType::FreezeBalanceContract)
                | Some(tron_backend_execution::TronContractType::UnfreezeBalanceContract)
                | Some(tron_backend_execution::TronContractType::FreezeBalanceV2Contract)
                | Some(tron_backend_execution::TronContractType::UnfreezeBalanceV2Contract)
                | Some(tron_backend_execution::TronContractType::WithdrawExpireUnfreezeContract)
                | Some(tron_backend_execution::TronContractType::DelegateResourceContract)
                | Some(tron_backend_execution::TronContractType::UndelegateResourceContract)
                | Some(tron_backend_execution::TronContractType::MarketSellAssetContract)
                | Some(tron_backend_execution::TronContractType::MarketCancelOrderContract)
        );
        let from = match strip_tron_address_prefix(&tx.from) {
            Ok(from_bytes) => revm_primitives::Address::from_slice(from_bytes),
            Err(e) if allow_malformed_from => {
                debug!(
                    "Allowing malformed from address for {:?}: {}",
                    contract_type, e
                );
                revm_primitives::Address::ZERO
            }
            Err(e) => return Err(e),
        };

        // Some NON_VM system contracts validate `to` raw bytes themselves using Java's
        // `DecodeUtil.addressValid` semantics (21 bytes + prefix match). If the gRPC conversion
        // fails for these contract types, we must not fail early — instead we store the raw bytes
        // in `to_raw` and let contract-level validation produce the correct Java error messages
        // and ordering.
        let allow_malformed_to = matches!(
            contract_type,
            Some(tron_backend_execution::TronContractType::TransferContract)
                | Some(tron_backend_execution::TronContractType::TransferAssetContract)
        );

        // Phase 0.5: Fix CreateSmartContract toAddress semantics
        // When tx_kind=VM and contract_type=CREATE_SMART_CONTRACT (30), Java sends a 20-byte
        // zero array for toAddress. Rust must treat this as None (contract creation), not
        // Some(Address::ZERO) which would be interpreted as a call to address 0.
        let to = if tx.to.is_empty() {
            None // Contract creation (empty to field)
        } else {
            match strip_tron_address_prefix(&tx.to) {
                Ok(to_bytes) => {
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
                }
                Err(e) if allow_malformed_to => {
                    debug!(
                        "Allowing malformed to address for {:?}: {}",
                        contract_type, e
                    );
                    // Contract-level validation will check to_raw and return "Invalid toAddress!"
                    None
                }
                Err(e) => return Err(e),
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
            debug!(
                "Using default gas limit {} for zero energy_limit transaction",
                default_limit
            );
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

        let contract_parameter = tx
            .contract_parameter
            .as_ref()
            .map(|any| TronContractParameter {
                type_url: any.type_url.clone(),
                value: any.value.clone(),
            });

        let to_raw = if !tx.to.is_empty() {
            Some(tx.to.clone())
        } else {
            None
        };

        let metadata = tron_backend_execution::TxMetadata {
            contract_type,
            asset_id,
            from_raw: Some(tx.from.clone()),
            to_raw,
            contract_parameter,
            transaction_bytes_size: if transaction_bytes_size > 0 {
                Some(transaction_bytes_size)
            } else {
                None
            },
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

        debug!(
            "Transaction kind: {:?}, contract_type: {:?}",
            tx_kind, transaction.metadata.contract_type
        );

        Ok((transaction, tx_kind))
    }

    pub(super) fn convert_protobuf_context(
        &self,
        ctx: Option<&crate::backend::ExecutionContext>,
    ) -> Result<TronExecutionContext, String> {
        let ctx = ctx.ok_or("Execution context is required")?;

        // Log the raw context data from Java
        debug!(
            "Raw context from Java - energy_limit: {}, energy_price: {}",
            ctx.energy_limit, ctx.energy_price
        );

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

    pub(super) fn convert_call_contract_request_to_transaction(
        &self,
        req: &crate::backend::CallContractRequest,
    ) -> Result<TronTransaction, String> {
        // close_loop iter 6: prefer the full `transaction` field when the
        // caller populates it. Producers updated after iter 6
        // (RemoteExecutionSPI + new ExecutionGrpcClientTest rows) set this
        // field so the server sees the same transaction shape as full
        // execution — value, gas_limit, contract_type, asset_id,
        // contract_parameter, etc. Older producers that only populate the
        // legacy from/to/data/context fields still work via the fallback
        // branch below.
        if let Some(tx_proto) = req.transaction.as_ref() {
            let (tron_tx, _tx_kind) =
                self.convert_protobuf_transaction(Some(tx_proto), 0)?;
            return Ok(tron_tx);
        }

        // Legacy fallback for producers that have not yet migrated to
        // setting `transaction`. Reconstructs a minimal TronTransaction
        // from the historical flat fields with the same hardcoded
        // defaults the pre-iter-6 converter used.
        let from_bytes = strip_tron_address_prefix(&req.from)?;
        let from = revm_primitives::Address::from_slice(from_bytes);

        let to_bytes = strip_tron_address_prefix(&req.to)?;
        let to = revm_primitives::Address::from_slice(to_bytes);

        Ok(TronTransaction {
            from,
            to: Some(to),
            value: revm_primitives::U256::ZERO, // Legacy: contract calls assumed non-payable
            data: req.data.clone().into(),
            gas_limit: 1000000, // Legacy: default gas limit for contract calls
            gas_price: revm_primitives::U256::from(1),
            nonce: 0, // Contract calls don't use nonce
            metadata: tron_backend_execution::TxMetadata::default(), // Legacy: no metadata
        })
    }

    /// Convert a TronExecutionResult to protobuf ExecuteTransactionResponse.
    ///
    /// # Arguments
    /// * `result` - The execution result from Rust backend
    /// * `pre_exec_aext` - Pre-execution AEXT values for hybrid mode
    /// * `touched_keys` - Optional list of database keys touched during execution (for B-镜像)
    /// * `write_mode` - The write mode: 0 = COMPUTE_ONLY, 1 = PERSISTED
    /// * `address_prefix` - The DB-detected network address prefix (0x41 mainnet, 0xa0 testnet).
    ///   Passed from the gRPC execution handler via `storage_adapter.address_prefix()`.
    ///   All emitted addresses in the response use this prefix so non-mainnet networks
    ///   do not lose their DB prefix at the protobuf/gRPC boundary.
    pub(super) fn convert_execution_result_to_protobuf(
        &self,
        result: TronExecutionResult,
        pre_exec_aext: &std::collections::HashMap<revm::primitives::Address, AccountAext>,
        touched_keys: Option<&[TouchedKey]>,
        write_mode: i32,
        address_prefix: u8,
    ) -> ExecuteTransactionResponse {
        let status = if result.success {
            execution_result::Status::Success
        } else {
            execution_result::Status::Revert
        };

        let logs: Vec<LogEntry> = result
            .logs
            .iter()
            .map(|log| LogEntry {
                address: add_tron_address_prefix_with(&log.address, address_prefix),
                topics: log.topics().iter().map(|t| t.as_slice().to_vec()).collect(),
                data: log.data.data.to_vec(),
            })
            .collect();

        let state_changes: Vec<StateChange> = result.state_changes.iter().map(|change| {
            match change {
                TronStateChange::StorageChange { address, key, old_value, new_value } => {
                    StateChange {
                        change: Some(crate::backend::state_change::Change::StorageChange(
                            crate::backend::StorageChange {
                                address: add_tron_address_prefix_with(address, address_prefix),
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
                                    debug!("Using pre-exec AEXT for address {} in hybrid mode", hex::encode(add_tron_address_prefix_with(addr, address_prefix)));
                                    // Use the same AEXT for both old and new (unchanged fields)
                                    (Some(aext.net_usage), Some(aext.free_net_usage), Some(aext.energy_usage),
                                     Some(aext.latest_consume_time), Some(aext.latest_consume_free_time),
                                     Some(aext.latest_consume_time_for_energy), Some(aext.net_window_size),
                                     Some(aext.net_window_optimized), Some(aext.energy_window_size),
                                     Some(aext.energy_window_optimized))
                                } else {
                                    // Not provided, fall back to defaults
                                    debug!("No pre-exec AEXT for address {}, using defaults in hybrid mode", hex::encode(add_tron_address_prefix_with(addr, address_prefix)));
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
                               aext_mode, is_eoa, hex::encode(add_tron_address_prefix_with(addr, address_prefix)),
                               net_window_size, energy_window_size);

                        crate::backend::AccountInfo {
                            address: add_tron_address_prefix_with(addr, address_prefix),
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
                                address: add_tron_address_prefix_with(address, address_prefix),
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
        let freeze_changes: Vec<crate::backend::FreezeLedgerChange> = result
            .freeze_changes
            .iter()
            .map(|change| {
                use crate::backend::freeze_ledger_change::Resource;
                use tron_backend_execution::FreezeLedgerResource;

                let resource = match change.resource {
                    FreezeLedgerResource::Bandwidth => Resource::Bandwidth,
                    FreezeLedgerResource::Energy => Resource::Energy,
                    FreezeLedgerResource::TronPower => Resource::TronPower,
                };

                crate::backend::FreezeLedgerChange {
                    owner_address: add_tron_address_prefix_with(&change.owner_address, address_prefix),
                    resource: resource as i32,
                    amount: change.amount,
                    expiration_ms: change.expiration_ms,
                    v2_model: change.v2_model,
                }
            })
            .collect();

        // Convert global resource totals changes from execution result to protobuf
        let global_resource_changes: Vec<crate::backend::GlobalResourceTotalsChange> = result
            .global_resource_changes
            .iter()
            .map(|change| crate::backend::GlobalResourceTotalsChange {
                total_net_weight: change.total_net_weight,
                total_net_limit: change.total_net_limit,
                total_energy_weight: change.total_energy_weight,
                total_energy_limit: change.total_energy_limit,
            })
            .collect();

        // Convert TRC-10 changes from execution result to protobuf (Phase 2)
        let trc10_changes: Vec<crate::backend::Trc10Change> = result
            .trc10_changes
            .iter()
            .map(|change| match change {
                tron_backend_execution::Trc10Change::AssetIssued(issued) => {
                    crate::backend::Trc10Change {
                        kind: Some(crate::backend::trc10_change::Kind::AssetIssued(
                            crate::backend::Trc10AssetIssued {
                                owner_address: add_tron_address_prefix_with(&issued.owner_address, address_prefix),
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
                            },
                        )),
                    }
                }
                tron_backend_execution::Trc10Change::AssetTransferred(transferred) => {
                    crate::backend::Trc10Change {
                        kind: Some(crate::backend::trc10_change::Kind::AssetTransferred(
                            crate::backend::Trc10AssetTransferred {
                                owner_address: add_tron_address_prefix_with(&transferred.owner_address, address_prefix),
                                to_address: add_tron_address_prefix_with(&transferred.to_address, address_prefix),
                                asset_name: transferred.asset_name.clone(),
                                token_id: transferred.token_id.clone().unwrap_or_default(),
                                amount: transferred.amount,
                            },
                        )),
                    }
                }
            })
            .collect();

        // Convert VoteChange from execution result to protobuf (Phase 2: Account.votes update)
        let vote_changes: Vec<crate::backend::VoteChange> = result
            .vote_changes
            .iter()
            .map(|change| crate::backend::VoteChange {
                owner_address: add_tron_address_prefix_with(&change.owner_address, address_prefix),
                votes: change
                    .votes
                    .iter()
                    .map(|v| crate::backend::Vote {
                        vote_address: add_tron_address_prefix_with(&v.vote_address, address_prefix),
                        vote_count: v.vote_count as i64,
                    })
                    .collect(),
            })
            .collect();

        // Convert WithdrawChange from execution result to protobuf (WithdrawBalanceContract sidecar)
        let withdraw_changes: Vec<crate::backend::WithdrawChange> = result
            .withdraw_changes
            .iter()
            .map(|change| crate::backend::WithdrawChange {
                owner_address: add_tron_address_prefix_with(&change.owner_address, address_prefix),
                amount: change.amount,
                latest_withdraw_time: change.latest_withdraw_time,
            })
            .collect();

        let error_message = result.error.unwrap_or_default();

        // Phase 2.I L2: Convert contract_address to TRON 21-byte format if present
        let contract_address_bytes = result
            .contract_address
            .map(|addr| add_tron_address_prefix_with(&addr, address_prefix))
            .unwrap_or_default();

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
                resource_usage: vec![],  // Not implemented yet
                freeze_changes,          // Converted from TronExecutionResult
                global_resource_changes, // Converted from TronExecutionResult
                trc10_changes,           // Phase 2: Converted TRC-10 semantic changes
                vote_changes,            // Phase 2: VoteChange for Account.votes update
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

#[cfg(test)]
mod tests {
    use super::BackendService;
    use crate::backend::{ContractType, TronTransaction as ProtoTx, TxKind};
    use tron_backend_common::ExecutionConfig;
    use tron_backend_common::ModuleManager;
    use tron_backend_execution::ExecutionModule;

    #[test]
    fn test_convert_protobuf_transaction_allows_empty_from_for_witness_create() {
        let config = ExecutionConfig::default();
        let mut module_manager = ModuleManager::new();
        module_manager.register("execution", Box::new(ExecutionModule::new(config)));

        let backend_service = BackendService::new(module_manager);

        let mut proto_tx = ProtoTx::default();
        proto_tx.from = vec![];
        proto_tx.tx_kind = TxKind::NonVm as i32;
        proto_tx.contract_type = ContractType::WitnessCreateContract as i32;

        let (transaction, tx_kind) = backend_service
            .convert_protobuf_transaction(Some(&proto_tx), 0)
            .unwrap();

        assert_eq!(tx_kind, TxKind::NonVm);
        assert_eq!(transaction.from, revm_primitives::Address::ZERO);
        assert_eq!(
            transaction.metadata.contract_type,
            Some(tron_backend_execution::TronContractType::WitnessCreateContract)
        );
        assert_eq!(transaction.metadata.from_raw, Some(vec![]));
    }

    #[test]
    fn test_convert_protobuf_transaction_allows_empty_from_for_witness_update() {
        let config = ExecutionConfig::default();
        let mut module_manager = ModuleManager::new();
        module_manager.register("execution", Box::new(ExecutionModule::new(config)));

        let backend_service = BackendService::new(module_manager);

        let mut proto_tx = ProtoTx::default();
        proto_tx.from = vec![];
        proto_tx.tx_kind = TxKind::NonVm as i32;
        proto_tx.contract_type = ContractType::WitnessUpdateContract as i32;

        let (transaction, tx_kind) = backend_service
            .convert_protobuf_transaction(Some(&proto_tx), 0)
            .unwrap();

        assert_eq!(tx_kind, TxKind::NonVm);
        assert_eq!(transaction.from, revm_primitives::Address::ZERO);
        assert_eq!(
            transaction.metadata.contract_type,
            Some(tron_backend_execution::TronContractType::WitnessUpdateContract)
        );
        assert_eq!(transaction.metadata.from_raw, Some(vec![]));
    }

    #[test]
    fn test_convert_protobuf_transaction_allows_empty_from_for_withdraw_expire_unfreeze() {
        let config = ExecutionConfig::default();
        let mut module_manager = ModuleManager::new();
        module_manager.register("execution", Box::new(ExecutionModule::new(config)));

        let backend_service = BackendService::new(module_manager);

        let mut proto_tx = ProtoTx::default();
        proto_tx.from = vec![];
        proto_tx.tx_kind = TxKind::NonVm as i32;
        proto_tx.contract_type = ContractType::WithdrawExpireUnfreezeContract as i32;
        proto_tx.contract_parameter = Some(prost_types::Any {
            type_url: "type.googleapis.com/protocol.WithdrawExpireUnfreezeContract".to_string(),
            value: vec![],
        });

        let (transaction, tx_kind) = backend_service
            .convert_protobuf_transaction(Some(&proto_tx), 0)
            .unwrap();

        assert_eq!(tx_kind, TxKind::NonVm);
        assert_eq!(transaction.from, revm_primitives::Address::ZERO);
        assert_eq!(
            transaction.metadata.contract_type,
            Some(tron_backend_execution::TronContractType::WithdrawExpireUnfreezeContract)
        );
        assert_eq!(transaction.metadata.from_raw, Some(vec![]));
        // Verify contract_parameter is preserved for contract-level validation
        assert!(transaction.metadata.contract_parameter.is_some());
        let param = transaction.metadata.contract_parameter.unwrap();
        assert!(param.type_url.ends_with("WithdrawExpireUnfreezeContract"));
    }

    // ---------- convert_execution_result_to_protobuf address-prefix tests ----------

    fn make_test_result_with_trc10_issued(
        owner: revm_primitives::Address,
    ) -> tron_backend_execution::TronExecutionResult {
        tron_backend_execution::TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used: 0,
            logs: vec![],
            state_changes: vec![],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![tron_backend_execution::Trc10Change::AssetIssued(
                tron_backend_execution::Trc10AssetIssued {
                    owner_address: owner,
                    name: b"TestToken".to_vec(),
                    abbr: b"TT".to_vec(),
                    total_supply: 1_000_000,
                    trx_num: 1,
                    precision: 6,
                    num: 1,
                    start_time: 0,
                    end_time: 0,
                    description: vec![],
                    url: vec![],
                    free_asset_net_limit: 0,
                    public_free_asset_net_limit: 0,
                    public_free_asset_net_usage: 0,
                    public_latest_free_net_time: 0,
                    token_id: Some("1000001".to_string()),
                },
            )],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: None,
            contract_address: None,
        }
    }

    fn make_test_result_with_trc10_transferred(
        owner: revm_primitives::Address,
        to: revm_primitives::Address,
    ) -> tron_backend_execution::TronExecutionResult {
        tron_backend_execution::TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used: 0,
            logs: vec![],
            state_changes: vec![],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![tron_backend_execution::Trc10Change::AssetTransferred(
                tron_backend_execution::Trc10AssetTransferred {
                    owner_address: owner,
                    to_address: to,
                    asset_name: b"TestToken".to_vec(),
                    token_id: Some("1000001".to_string()),
                    amount: 100,
                },
            )],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: None,
            contract_address: None,
        }
    }

    fn make_backend_service() -> BackendService {
        let config = ExecutionConfig::default();
        let mut module_manager = ModuleManager::new();
        module_manager.register("execution", Box::new(ExecutionModule::new(config)));
        BackendService::new(module_manager)
    }

    #[test]
    fn test_convert_result_trc10_issued_uses_testnet_prefix() {
        let service = make_backend_service();
        let owner = revm_primitives::Address::repeat_byte(0xAB);
        let result = make_test_result_with_trc10_issued(owner);
        let empty_aext = std::collections::HashMap::new();

        let response = service.convert_execution_result_to_protobuf(
            result, &empty_aext, None, 0, 0xa0,
        );

        let trc10 = &response.result.unwrap().trc10_changes;
        assert_eq!(trc10.len(), 1);
        if let Some(crate::backend::trc10_change::Kind::AssetIssued(issued)) = &trc10[0].kind {
            assert_eq!(issued.owner_address.len(), 21);
            assert_eq!(issued.owner_address[0], 0xa0, "Expected testnet prefix 0xa0");
            assert_eq!(&issued.owner_address[1..], owner.as_slice());
        } else {
            panic!("Expected AssetIssued variant");
        }
    }

    #[test]
    fn test_convert_result_trc10_issued_uses_mainnet_prefix() {
        let service = make_backend_service();
        let owner = revm_primitives::Address::repeat_byte(0xAB);
        let result = make_test_result_with_trc10_issued(owner);
        let empty_aext = std::collections::HashMap::new();

        let response = service.convert_execution_result_to_protobuf(
            result, &empty_aext, None, 0, 0x41,
        );

        let trc10 = &response.result.unwrap().trc10_changes;
        assert_eq!(trc10.len(), 1);
        if let Some(crate::backend::trc10_change::Kind::AssetIssued(issued)) = &trc10[0].kind {
            assert_eq!(issued.owner_address[0], 0x41, "Expected mainnet prefix 0x41");
        } else {
            panic!("Expected AssetIssued variant");
        }
    }

    #[test]
    fn test_convert_result_trc10_transferred_uses_testnet_prefix() {
        let service = make_backend_service();
        let owner = revm_primitives::Address::repeat_byte(0xAB);
        let to = revm_primitives::Address::repeat_byte(0xCD);
        let result = make_test_result_with_trc10_transferred(owner, to);
        let empty_aext = std::collections::HashMap::new();

        let response = service.convert_execution_result_to_protobuf(
            result, &empty_aext, None, 0, 0xa0,
        );

        let trc10 = &response.result.unwrap().trc10_changes;
        assert_eq!(trc10.len(), 1);
        if let Some(crate::backend::trc10_change::Kind::AssetTransferred(transferred)) = &trc10[0].kind {
            assert_eq!(transferred.owner_address.len(), 21);
            assert_eq!(transferred.owner_address[0], 0xa0, "owner_address: expected testnet prefix 0xa0");
            assert_eq!(&transferred.owner_address[1..], owner.as_slice());

            assert_eq!(transferred.to_address.len(), 21);
            assert_eq!(transferred.to_address[0], 0xa0, "to_address: expected testnet prefix 0xa0");
            assert_eq!(&transferred.to_address[1..], to.as_slice());
        } else {
            panic!("Expected AssetTransferred variant");
        }
    }

    #[test]
    fn test_convert_result_logs_use_address_prefix() {
        // Verify that log addresses also respect the address_prefix parameter
        let service = make_backend_service();
        let log_addr = revm_primitives::Address::repeat_byte(0x11);
        let result = tron_backend_execution::TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used: 0,
            logs: vec![revm_primitives::Log::new_unchecked(log_addr, vec![], revm_primitives::Bytes::new())],
            state_changes: vec![],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: None,
            contract_address: None,
        };
        let empty_aext = std::collections::HashMap::new();

        let response = service.convert_execution_result_to_protobuf(
            result, &empty_aext, None, 0, 0xa0,
        );

        let logs = &response.result.unwrap().logs;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].address.len(), 21);
        assert_eq!(logs[0].address[0], 0xa0, "Log address should use testnet prefix");
    }

    // ---------- address-prefix tests for state_changes, freeze, vote, withdraw, contract_address ----------

    #[test]
    fn test_convert_result_storage_change_uses_address_prefix() {
        let service = make_backend_service();
        let addr = revm_primitives::Address::repeat_byte(0x22);
        let result = tron_backend_execution::TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used: 0,
            logs: vec![],
            state_changes: vec![tron_backend_execution::TronStateChange::StorageChange {
                address: addr,
                key: revm_primitives::U256::from(1),
                old_value: revm_primitives::U256::ZERO,
                new_value: revm_primitives::U256::from(42),
            }],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: None,
            contract_address: None,
        };
        let empty_aext = std::collections::HashMap::new();

        let response = service.convert_execution_result_to_protobuf(
            result, &empty_aext, None, 0, 0xa0,
        );

        let state_changes = &response.result.unwrap().state_changes;
        assert_eq!(state_changes.len(), 1);
        if let Some(crate::backend::state_change::Change::StorageChange(sc)) = &state_changes[0].change {
            assert_eq!(sc.address.len(), 21);
            assert_eq!(sc.address[0], 0xa0, "StorageChange address should use testnet prefix");
            assert_eq!(&sc.address[1..], addr.as_slice());
        } else {
            panic!("Expected StorageChange variant");
        }
    }

    #[test]
    fn test_convert_result_account_change_uses_address_prefix() {
        let service = make_backend_service();
        let addr = revm_primitives::Address::repeat_byte(0x33);
        let old_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(100),
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };
        let new_info = revm_primitives::AccountInfo {
            balance: revm_primitives::U256::from(200),
            nonce: 1,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };
        let result = tron_backend_execution::TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used: 0,
            logs: vec![],
            state_changes: vec![tron_backend_execution::TronStateChange::AccountChange {
                address: addr,
                old_account: Some(old_info),
                new_account: Some(new_info),
            }],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: None,
            contract_address: None,
        };
        let empty_aext = std::collections::HashMap::new();

        let response = service.convert_execution_result_to_protobuf(
            result, &empty_aext, None, 0, 0xa0,
        );

        let state_changes = &response.result.unwrap().state_changes;
        assert_eq!(state_changes.len(), 1);
        if let Some(crate::backend::state_change::Change::AccountChange(ac)) = &state_changes[0].change {
            // Outer address
            assert_eq!(ac.address.len(), 21);
            assert_eq!(ac.address[0], 0xa0, "AccountChange address should use testnet prefix");
            assert_eq!(&ac.address[1..], addr.as_slice());
            // Inner old_account address
            let old_acc = ac.old_account.as_ref().unwrap();
            assert_eq!(old_acc.address[0], 0xa0, "old_account address should use testnet prefix");
            // Inner new_account address
            let new_acc = ac.new_account.as_ref().unwrap();
            assert_eq!(new_acc.address[0], 0xa0, "new_account address should use testnet prefix");
        } else {
            panic!("Expected AccountChange variant");
        }
    }

    #[test]
    fn test_convert_result_freeze_change_uses_address_prefix() {
        let service = make_backend_service();
        let addr = revm_primitives::Address::repeat_byte(0x44);
        let result = tron_backend_execution::TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used: 0,
            logs: vec![],
            state_changes: vec![],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![tron_backend_execution::FreezeLedgerChange {
                owner_address: addr,
                resource: tron_backend_execution::FreezeLedgerResource::Bandwidth,
                amount: 1000,
                expiration_ms: 0,
                v2_model: true,
            }],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: None,
            contract_address: None,
        };
        let empty_aext = std::collections::HashMap::new();

        let response = service.convert_execution_result_to_protobuf(
            result, &empty_aext, None, 0, 0xa0,
        );

        let freeze = &response.result.unwrap().freeze_changes;
        assert_eq!(freeze.len(), 1);
        assert_eq!(freeze[0].owner_address.len(), 21);
        assert_eq!(freeze[0].owner_address[0], 0xa0, "FreezeLedgerChange address should use testnet prefix");
        assert_eq!(&freeze[0].owner_address[1..], addr.as_slice());
    }

    #[test]
    fn test_convert_result_vote_change_uses_address_prefix() {
        let service = make_backend_service();
        let voter = revm_primitives::Address::repeat_byte(0x55);
        let witness = revm_primitives::Address::repeat_byte(0x66);
        let result = tron_backend_execution::TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used: 0,
            logs: vec![],
            state_changes: vec![],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![tron_backend_execution::VoteChange {
                owner_address: voter,
                votes: vec![tron_backend_execution::VoteEntry {
                    vote_address: witness,
                    vote_count: 100,
                }],
            }],
            withdraw_changes: vec![],
            tron_transaction_result: None,
            contract_address: None,
        };
        let empty_aext = std::collections::HashMap::new();

        let response = service.convert_execution_result_to_protobuf(
            result, &empty_aext, None, 0, 0xa0,
        );

        let votes = &response.result.unwrap().vote_changes;
        assert_eq!(votes.len(), 1);
        assert_eq!(votes[0].owner_address.len(), 21);
        assert_eq!(votes[0].owner_address[0], 0xa0, "VoteChange owner_address should use testnet prefix");
        assert_eq!(&votes[0].owner_address[1..], voter.as_slice());
        // Check nested vote_address
        assert_eq!(votes[0].votes.len(), 1);
        assert_eq!(votes[0].votes[0].vote_address.len(), 21);
        assert_eq!(votes[0].votes[0].vote_address[0], 0xa0, "Vote vote_address should use testnet prefix");
        assert_eq!(&votes[0].votes[0].vote_address[1..], witness.as_slice());
    }

    #[test]
    fn test_convert_result_withdraw_change_uses_address_prefix() {
        let service = make_backend_service();
        let addr = revm_primitives::Address::repeat_byte(0x77);
        let result = tron_backend_execution::TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used: 0,
            logs: vec![],
            state_changes: vec![],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![tron_backend_execution::WithdrawChange {
                owner_address: addr,
                amount: 5000,
                latest_withdraw_time: 1700000000000,
            }],
            tron_transaction_result: None,
            contract_address: None,
        };
        let empty_aext = std::collections::HashMap::new();

        let response = service.convert_execution_result_to_protobuf(
            result, &empty_aext, None, 0, 0xa0,
        );

        let withdrawals = &response.result.unwrap().withdraw_changes;
        assert_eq!(withdrawals.len(), 1);
        assert_eq!(withdrawals[0].owner_address.len(), 21);
        assert_eq!(withdrawals[0].owner_address[0], 0xa0, "WithdrawChange address should use testnet prefix");
        assert_eq!(&withdrawals[0].owner_address[1..], addr.as_slice());
    }

    #[test]
    fn test_convert_result_contract_address_uses_address_prefix() {
        let service = make_backend_service();
        let contract_addr = revm_primitives::Address::repeat_byte(0x88);
        let result = tron_backend_execution::TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(),
            energy_used: 0,
            bandwidth_used: 0,
            logs: vec![],
            state_changes: vec![],
            error: None,
            aext_map: std::collections::HashMap::new(),
            freeze_changes: vec![],
            global_resource_changes: vec![],
            trc10_changes: vec![],
            vote_changes: vec![],
            withdraw_changes: vec![],
            tron_transaction_result: None,
            contract_address: Some(contract_addr),
        };
        let empty_aext = std::collections::HashMap::new();

        let response = service.convert_execution_result_to_protobuf(
            result, &empty_aext, None, 0, 0xa0,
        );

        let ca = &response.result.unwrap().contract_address;
        assert_eq!(ca.len(), 21);
        assert_eq!(ca[0], 0xa0, "contract_address should use testnet prefix");
        assert_eq!(&ca[1..], contract_addr.as_slice());
    }

    #[test]
    fn test_convert_protobuf_transaction_allows_empty_from_for_freeze_v2_family() {
        // Test that all freeze-v2 related contracts allow malformed from addresses
        let contract_types = vec![
            (
                ContractType::FreezeBalanceV2Contract,
                tron_backend_execution::TronContractType::FreezeBalanceV2Contract,
            ),
            (
                ContractType::UnfreezeBalanceV2Contract,
                tron_backend_execution::TronContractType::UnfreezeBalanceV2Contract,
            ),
            (
                ContractType::DelegateResourceContract,
                tron_backend_execution::TronContractType::DelegateResourceContract,
            ),
            (
                ContractType::UndelegateResourceContract,
                tron_backend_execution::TronContractType::UndelegateResourceContract,
            ),
            (
                ContractType::FreezeBalanceContract,
                tron_backend_execution::TronContractType::FreezeBalanceContract,
            ),
            (
                ContractType::UnfreezeBalanceContract,
                tron_backend_execution::TronContractType::UnfreezeBalanceContract,
            ),
        ];

        for (proto_ct, expected_ct) in contract_types {
            let config = ExecutionConfig::default();
            let mut module_manager = ModuleManager::new();
            module_manager.register("execution", Box::new(ExecutionModule::new(config)));
            let backend_service = BackendService::new(module_manager);

            let mut proto_tx = ProtoTx::default();
            proto_tx.from = vec![];
            proto_tx.tx_kind = TxKind::NonVm as i32;
            proto_tx.contract_type = proto_ct as i32;

            let result = backend_service.convert_protobuf_transaction(Some(&proto_tx), 0);
            assert!(
                result.is_ok(),
                "Contract type {:?} should allow empty from, but got error: {:?}",
                expected_ct,
                result.err()
            );
            let (transaction, _) = result.unwrap();
            assert_eq!(transaction.from, revm_primitives::Address::ZERO);
            assert_eq!(transaction.metadata.contract_type, Some(expected_ct));
        }
    }

    // =========================================================================
    // iter 6: CallContractRequest proto reshape tests
    // =========================================================================
    //
    // These tests lock the iter-6 behavior of
    // `convert_call_contract_request_to_transaction`:
    //   - when `CallContractRequest.transaction` is set, the converter
    //     uses it and carries value / gas_limit / metadata through;
    //   - when `CallContractRequest.transaction` is absent, the
    //     converter falls back to the legacy `from`/`to`/`data` flat
    //     fields with the historical hardcoded defaults.
    //
    // The goal is to prevent a future refactor from silently removing
    // the fallback (which would break pre-iter-6 clients) or from
    // silently ignoring the `transaction` field (which would re-open
    // the iter-5 correctness gap).

    fn make_test_backend_service() -> BackendService {
        let config = ExecutionConfig::default();
        let mut module_manager = ModuleManager::new();
        module_manager.register("execution", Box::new(ExecutionModule::new(config)));
        BackendService::new(module_manager)
    }

    fn tron_prefixed(addr: [u8; 20]) -> Vec<u8> {
        let mut v = Vec::with_capacity(21);
        v.push(0x41);
        v.extend_from_slice(&addr);
        v
    }

    #[test]
    fn call_contract_request_prefers_transaction_field_when_present() {
        // When `transaction` is populated, the converter must use it
        // (via convert_protobuf_transaction) and the legacy flat
        // fields must be ignored. This locks the iter-6 "preferred"
        // path so a refactor cannot accidentally drop it.
        let backend_service = make_test_backend_service();

        // Build a nested TronTransaction carrying fields that the
        // legacy fallback path would drop or hardcode:
        //   - non-zero value (TronTransaction.value is bytes, 32-byte BE)
        //   - non-default energy_limit (→ gas_limit)
        //   - non-default tx_kind
        //   - non-default contract_type
        let inner_from = tron_prefixed([0xAA; 20]);
        let inner_to = tron_prefixed([0xBB; 20]);
        // 1000 in 32-byte big-endian = 31 zero bytes + 0xE8
        let mut value_be = vec![0u8; 32];
        value_be[31] = 0xE8;
        value_be[30] = 0x03;
        let mut inner_tx = ProtoTx::default();
        inner_tx.from = inner_from.clone();
        inner_tx.to = inner_to.clone();
        inner_tx.data = vec![0x01, 0x02, 0x03];
        inner_tx.value = value_be;
        inner_tx.energy_limit = 4242;
        inner_tx.tx_kind = TxKind::Vm as i32;
        inner_tx.contract_type = ContractType::TriggerSmartContract as i32;

        // Intentionally give the legacy flat fields DIFFERENT values
        // so we can prove the converter picked the nested transaction
        // instead of reading the flat fields.
        let legacy_from = tron_prefixed([0xCC; 20]);
        let legacy_to = tron_prefixed([0xDD; 20]);
        let request = crate::backend::CallContractRequest {
            from: legacy_from.clone(),
            to: legacy_to.clone(),
            data: vec![0xFF],
            context: None,
            transaction: Some(inner_tx),
        };

        let tron_tx = backend_service
            .convert_call_contract_request_to_transaction(&request)
            .expect("convert with transaction should succeed");

        // Address should come from the nested transaction (0xAA...),
        // not the legacy flat field (0xCC...).
        assert_eq!(tron_tx.from.as_slice(), &[0xAAu8; 20]);
        assert_eq!(tron_tx.to.expect("to present").as_slice(), &[0xBBu8; 20]);
        assert_eq!(tron_tx.data.as_ref(), &[0x01, 0x02, 0x03]);
        // Value must reflect the nested transaction (1000), NOT the
        // legacy hardcoded U256::ZERO.
        assert_eq!(tron_tx.value, revm_primitives::U256::from(1000u64));
        // gas_limit should reflect energy_limit=4242 from the nested
        // transaction, NOT the legacy hardcoded default of 1_000_000.
        assert_eq!(tron_tx.gas_limit, 4242);
        assert_eq!(
            tron_tx.metadata.contract_type,
            Some(tron_backend_execution::TronContractType::TriggerSmartContract)
        );
    }

    #[test]
    fn call_contract_request_falls_back_to_legacy_fields_when_transaction_absent() {
        // When `transaction` is None, the converter must reconstruct a
        // minimal TronTransaction from the legacy flat fields with the
        // historical hardcoded defaults. This locks backward
        // compatibility with pre-iter-6 clients such as
        // `ExecutionGrpcClientTest.testCallContractRequestCreation`.
        let backend_service = make_test_backend_service();

        let legacy_from = tron_prefixed([0x11; 20]);
        let legacy_to = tron_prefixed([0x22; 20]);
        let request = crate::backend::CallContractRequest {
            from: legacy_from.clone(),
            to: legacy_to.clone(),
            data: vec![0xAB, 0xCD],
            context: None,
            transaction: None,
        };

        let tron_tx = backend_service
            .convert_call_contract_request_to_transaction(&request)
            .expect("legacy fallback should succeed");

        assert_eq!(tron_tx.from.as_slice(), &[0x11u8; 20]);
        assert_eq!(tron_tx.to.expect("to present").as_slice(), &[0x22u8; 20]);
        assert_eq!(tron_tx.data.as_ref(), &[0xAB, 0xCD]);
        // The legacy path hardcodes value=0 and gas_limit=1_000_000.
        assert_eq!(tron_tx.value, revm_primitives::U256::ZERO);
        assert_eq!(tron_tx.gas_limit, 1_000_000);
        // Legacy path uses default TxMetadata — no contract_type.
        assert_eq!(tron_tx.metadata.contract_type, None);
    }

    #[test]
    fn call_contract_request_legacy_fallback_rejects_malformed_address() {
        // The legacy fallback path should still validate the flat
        // address fields. A malformed `from` must be rejected with an
        // explicit error, matching the pre-iter-6 behavior.
        let backend_service = make_test_backend_service();

        let request = crate::backend::CallContractRequest {
            from: vec![0x00, 0x01, 0x02], // too short
            to: tron_prefixed([0x22; 20]),
            data: vec![],
            context: None,
            transaction: None,
        };

        let err = backend_service
            .convert_call_contract_request_to_transaction(&request)
            .unwrap_err();
        assert!(
            err.contains("Invalid address length"),
            "unexpected error: {err}"
        );
    }

    // =========================================================================
    // iter 6: CallContractResponse.Status enum is wired correctly
    // =========================================================================
    //
    // These tests lock the numeric mapping between the Rust-generated
    // prost enum and the proto wire codes. If someone reorders or
    // renumbers the enum in backend.proto, these tests will fail loudly
    // instead of letting the Java bridge silently mis-classify
    // responses.

    #[test]
    fn call_contract_response_status_wire_values_are_stable() {
        use crate::backend::call_contract_response::Status;
        assert_eq!(Status::Unspecified as i32, 0);
        assert_eq!(Status::Success as i32, 1);
        assert_eq!(Status::Revert as i32, 2);
        assert_eq!(Status::Halt as i32, 3);
        assert_eq!(Status::HandlerError as i32, 4);
    }
}

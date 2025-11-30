// Conversion helpers between protobuf and execution types
// Functions for converting transactions, contexts, and results between Java and Rust formats

use std::collections::HashMap;
use tracing::{debug, warn};
use revm_primitives::hex;
use tron_backend_execution::{TronTransaction, TronExecutionContext, TronExecutionResult, TronStateChange, AccountAext};
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

        // Convert bytes to Address (strip Tron 0x41 prefix if present)
        let from_bytes = strip_tron_address_prefix(&tx.from)?;
        let from = revm_primitives::Address::from_slice(from_bytes);

        let to = if tx.to.is_empty() {
            None // Contract creation
        } else {
            let to_bytes = strip_tron_address_prefix(&tx.to)?;
            Some(revm_primitives::Address::from_slice(to_bytes))
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

        // Extract contract_type and asset_id from protobuf for TRON system contracts
        let contract_type = if tx.contract_type != 0 {
            match tron_backend_execution::TronContractType::try_from(tx.contract_type) {
                Ok(ct) => {
                    debug!("Parsed contract type: {:?}", ct);
                    Some(ct)
                },
                Err(e) => {
                    warn!("Invalid contract type {}: {}, ignoring", tx.contract_type, e);
                    None
                }
            }
        } else {
            None
        };

        let asset_id = if !tx.asset_id.is_empty() {
            debug!("Parsed asset_id: {} bytes", tx.asset_id.len());
            Some(tx.asset_id.clone())
        } else {
            None
        };

        let metadata = tron_backend_execution::TxMetadata {
            contract_type,
            asset_id,
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

        // Extract tx_kind from protobuf, default to VM for backward compatibility
        let tx_kind = crate::backend::TxKind::try_from(tx.tx_kind).unwrap_or(crate::backend::TxKind::Vm);
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

        Ok(TronExecutionContext {
            block_number: ctx.block_number as u64,
            block_timestamp: ctx.block_timestamp as u64,
            block_coinbase,
            block_difficulty: revm_primitives::U256::from(1u64), // Default difficulty
            block_gas_limit,
            chain_id: 0x2b6653dc, // Tron mainnet chain ID
            energy_price: ctx.energy_price as u64,
            bandwidth_price: 1000, // Default bandwidth price
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

    pub(super) fn convert_execution_result_to_protobuf(
        &self,
        result: TronExecutionResult,
        pre_exec_aext: &std::collections::HashMap<revm::primitives::Address, AccountAext>
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
                }
            }
        }).collect();

        // Convert delegation changes from execution result to protobuf (Phase 2: delegation parity)
        let delegation_changes: Vec<crate::backend::DelegationChange> = result.delegation_changes.iter().map(|change| {
            use crate::backend::delegation_change::{Resource, Operation};
            use tron_backend_execution::DelegationOp;

            let resource = match change.resource {
                0 => Resource::Bandwidth,
                1 => Resource::Energy,
                _ => Resource::Bandwidth, // Default to bandwidth for unknown
            };

            let op = match change.op {
                DelegationOp::Add => Operation::Add,
                DelegationOp::Remove => Operation::Remove,
                DelegationOp::Unlock => Operation::Unlock,
            };

            crate::backend::DelegationChange {
                from_address: add_tron_address_prefix(&change.from),
                to_address: add_tron_address_prefix(&change.to),
                resource: resource as i32,
                amount: change.amount,
                expire_time_ms: change.expire_time_ms,
                v2_model: change.v2_model,
                op: op as i32,
            }
        }).collect();

        let error_message = result.error.unwrap_or_default();

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
                delegation_changes, // Phase 2: Converted delegation changes
            }),
            success: result.success,
            error_message,
        }
    }
}

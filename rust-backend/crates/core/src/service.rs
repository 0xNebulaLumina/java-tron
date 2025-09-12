use std::collections::HashMap;
use std::time::SystemTime;

use tonic::{Request, Response, Status};
use tracing::{info, error, debug, warn};
use tokio_stream::wrappers::ReceiverStream;
use tokio::sync::mpsc;

use tron_backend_common::{ModuleManager, HealthStatus, from_tron_address};
use tron_backend_execution::{TronTransaction, TronExecutionContext, TronExecutionResult, TronStateChange, ExecutionModule, StorageAdapter};
use crate::backend::*;

pub struct BackendService {
    module_manager: ModuleManager,
    start_time: SystemTime,
}

impl BackendService {
    pub fn new(module_manager: ModuleManager) -> Self {
        Self {
            module_manager,
            start_time: SystemTime::now(),
        }
    }
    
    fn get_storage_module(&self) -> Result<&Box<dyn tron_backend_common::Module>, Status> {
        self.module_manager.get("storage")
            .ok_or_else(|| Status::unavailable("Storage module not available"))
    }
    
    fn get_execution_module(&self) -> Result<&Box<dyn tron_backend_common::Module>, Status> {
        self.module_manager.get("execution")
            .ok_or_else(|| Status::unavailable("Execution module not available"))
    }
    
    fn get_storage_engine(&self) -> Result<&tron_backend_storage::StorageEngine, Status> {
        let storage_module = self.get_storage_module()?;
        
        // Downcast to the concrete storage module type
        let storage_module = storage_module
            .as_any()
            .downcast_ref::<tron_backend_storage::StorageModule>()
            .ok_or_else(|| Status::internal("Failed to downcast storage module"))?;
            
        storage_module.engine()
            .map_err(|e| Status::internal(format!("Storage engine not available: {}", e)))
    }
    
    fn get_execution_config(&self) -> Result<&tron_backend_common::ExecutionConfig, String> {
        let execution_module = self.get_execution_module()
            .map_err(|e| format!("Failed to get execution module: {}", e))?;
        
        // Downcast to the concrete execution module type
        let execution_module = execution_module
            .as_any()
            .downcast_ref::<ExecutionModule>()
            .ok_or_else(|| "Failed to downcast execution module".to_string())?;
        
        // Access the config field (we need to add a getter method)
        execution_module.get_config()
    }
    
    /// Detect if a transaction is likely a non-VM transaction based on heuristics
    fn is_likely_non_vm_transaction(&self, tx: &TronTransaction, storage_adapter: &tron_backend_execution::StorageModuleAdapter) -> bool {
        // Non-VM heuristic: empty data AND to address has no code
        if !tx.data.is_empty() {
            return false; // Has data, likely VM transaction
        }
        
        if tx.to.is_none() {
            return false; // Contract creation, definitely VM transaction
        }
        
        let to_address = tx.to.unwrap();
        
        // Check if the 'to' address has code (making it a contract)
        // We'll query the 'code' database to see if there's code at this address
        match storage_adapter.get_code(&to_address) {
            Ok(Some(code)) => {
                if code.is_empty() {
                    // No code = EOA (Externally Owned Account), likely non-VM transaction
                    true
                } else {
                    // Has code = Contract, VM transaction
                    false
                }
            },
            Ok(None) => {
                // No code entry = new or EOA account, likely non-VM transaction
                true
            },
            Err(_) => {
                // Error accessing code, be conservative and assume VM transaction
                false
            }
        }
    }
    
    /// Apply TRON fee policy post-processing to execution results
    fn apply_fee_post_processing(&self, 
                                result: &mut TronExecutionResult, 
                                tx: &TronTransaction, 
                                context: &TronExecutionContext, 
                                is_non_vm: bool) -> Result<(), String> {
        let execution_config = self.get_execution_config()?;
        let fee_config = &execution_config.fees;
        
        // Only apply fee post-processing in blackhole mode
        if fee_config.mode != "blackhole" {
            debug!("Fee mode is '{}', skipping fee post-processing", fee_config.mode);
            return Ok(());
        }
        
        // Validate blackhole address if required
        if fee_config.blackhole_address_base58.is_empty() {
            warn!("Fee mode is 'blackhole' but no blackhole address configured, skipping fee emission");
            return Ok(());
        }
        
        // Parse blackhole address
        let blackhole_evm_address = match from_tron_address(&fee_config.blackhole_address_base58) {
            Ok(addr) => revm_primitives::Address::from_slice(&addr),
            Err(e) => {
                warn!("Invalid blackhole address '{}': {}, skipping fee emission", 
                      fee_config.blackhole_address_base58, e);
                return Ok(());
            }
        };
        
        let mut fee_amount = 0u64;
        let mut should_emit = false;
        
        if is_non_vm {
            // Non-VM transaction fee handling
            if let Some(flat_fee) = fee_config.non_vm_blackhole_credit_flat {
                fee_amount = flat_fee;
                should_emit = true;
                debug!("Non-VM transaction: will emit flat fee {} SUN to blackhole", flat_fee);
            } else {
                debug!("Non-VM transaction: no flat fee configured, skipping fee emission");
            }
        } else {
            // VM transaction fee handling
            if fee_config.experimental_vm_blackhole_credit {
                // Approximate fee calculation: energy_used * energy_price
                fee_amount = result.energy_used * context.energy_price;
                should_emit = true;
                debug!("VM transaction: will emit estimated fee {} SUN ({}*{}) to blackhole", 
                       fee_amount, result.energy_used, context.energy_price);
            } else {
                debug!("VM transaction: experimental_vm_blackhole_credit disabled, skipping fee emission");
            }
        }
        
        if should_emit && fee_amount > 0 {
            // Create synthetic AccountChange for blackhole credit
            let blackhole_change = TronStateChange::AccountChange {
                address: blackhole_evm_address,
                old_account: None, // We don't know the old state, this is a synthetic credit
                new_account: Some(revm_primitives::AccountInfo {
                    balance: revm_primitives::U256::from(fee_amount), // This would be added to existing balance
                    nonce: 0,
                    code_hash: revm_primitives::B256::ZERO,
                    code: None,
                }),
            };
            
            result.state_changes.push(blackhole_change);
            debug!("Added synthetic blackhole fee credit: {} SUN to address {:?}", 
                   fee_amount, blackhole_evm_address);
            
            // Log warning about approximation
            warn!("Emitted synthetic fee credit to blackhole (approximation until Phase 3)");
        }
        
        Ok(())
    }
    
    /// Execute a non-VM transaction natively without EVM
    /// Handles TRON value transfer with proper fee accounting
    fn execute_non_vm_transaction(
        &self,
        storage_adapter: &tron_backend_execution::StorageModuleAdapter,
        transaction: &TronTransaction,
        context: &TronExecutionContext,
    ) -> Result<TronExecutionResult, String> {
        debug!("Executing non-VM transaction: from={:?}, to={:?}, value={}", 
               transaction.from, transaction.to, transaction.value);
        
        let execution_config = self.get_execution_config()?;
        let fee_config = &execution_config.fees;
        
        // For non-VM transactions, we need the 'to' address
        let to_address = transaction.to.ok_or("Non-VM transaction must have 'to' address")?;
        
        // Calculate bandwidth used based on transaction payload size
        let bandwidth_used = Self::calculate_bandwidth_usage(transaction);
        
        // Start with empty state changes
        let mut state_changes = Vec::new();
        
        // Load sender account
        let sender_account = storage_adapter.get_account(&transaction.from)
            .map_err(|e| format!("Failed to load sender account: {}", e))?
            .unwrap_or_default();
        
        // Load recipient account  
        let recipient_account = storage_adapter.get_account(&to_address)
            .map_err(|e| format!("Failed to load recipient account: {}", e))?
            .unwrap_or_default();
        
        // Phase 3 Fix: Only calculate fee if explicitly configured for non-VM transactions
        let fee_amount = match fee_config.non_vm_blackhole_credit_flat {
            Some(flat_fee) => {
                debug!("Using configured flat fee for non-VM: {} SUN", flat_fee);
                flat_fee
            },
            None => {
                // Phase 3 Fix: Default to no forced TRX fee for non-VM transactions
                // TRON uses free bandwidth first; only charge TRX when free bandwidth is insufficient
                debug!("No flat fee configured for non-VM, using 0 (TRON free bandwidth semantics)");
                0
            }
        };
        
        // Validate sender has enough balance for value + fee (only if fee > 0)
        let total_cost = transaction.value.checked_add(revm_primitives::U256::from(fee_amount))
            .ok_or("Value + fee overflow")?;
        
        if sender_account.balance < total_cost {
            return Err(format!("Insufficient balance: need {}, have {}", total_cost, sender_account.balance));
        }
        
        // Update sender account: balance -= (value + fee)
        let new_sender_balance = sender_account.balance - total_cost;
        let new_sender_account = revm_primitives::AccountInfo {
            balance: new_sender_balance,
            nonce: sender_account.nonce, // Phase 3 Fix: Do NOT increment nonce for non-VM TRX transfers
            code_hash: sender_account.code_hash,
            code: sender_account.code.clone(),
        };
        
        // Add sender account change
        state_changes.push(TronStateChange::AccountChange {
            address: transaction.from,
            old_account: Some(sender_account),
            new_account: Some(new_sender_account),
        });
        
        // Update recipient account: balance += value
        let new_recipient_balance = recipient_account.balance + transaction.value;
        let new_recipient_account = revm_primitives::AccountInfo {
            balance: new_recipient_balance,
            nonce: recipient_account.nonce,
            code_hash: recipient_account.code_hash,
            code: recipient_account.code.clone(),
        };
        
        // Add recipient account change
        let old_recipient_account = if recipient_account.balance.is_zero() && recipient_account.nonce == 0 {
            None // Account creation
        } else {
            Some(recipient_account)
        };
        
        state_changes.push(TronStateChange::AccountChange {
            address: to_address,
            old_account: old_recipient_account,
            new_account: Some(new_recipient_account),
        });
        
        // Handle fee based on configuration (only if fee_amount > 0)
        if fee_amount > 0 {
            match fee_config.mode.as_str() {
                "burn" => {
                    debug!("Burning fee {} SUN (no account delta for burn)", fee_amount);
                    // No additional state change - fee is burned (supply reduction)
                },
                "blackhole" => {
                    if !fee_config.blackhole_address_base58.is_empty() {
                        // Parse blackhole address and credit it
                        match tron_backend_common::from_tron_address(&fee_config.blackhole_address_base58) {
                            Ok(blackhole_bytes) => {
                                let blackhole_address = revm_primitives::Address::from_slice(&blackhole_bytes);
                                
                                // Load blackhole account
                                let blackhole_account = storage_adapter.get_account(&blackhole_address)
                                    .map_err(|e| format!("Failed to load blackhole account: {}", e))?
                                    .unwrap_or_default();
                                
                                // Credit blackhole account with fee
                                let new_blackhole_balance = blackhole_account.balance + revm_primitives::U256::from(fee_amount);
                                let new_blackhole_account = revm_primitives::AccountInfo {
                                    balance: new_blackhole_balance,
                                    nonce: blackhole_account.nonce,
                                    code_hash: blackhole_account.code_hash,
                                    code: blackhole_account.code.clone(),
                                };
                                
                                // Add blackhole account change
                                let old_blackhole_account = if blackhole_account.balance.is_zero() && blackhole_account.nonce == 0 {
                                    None // Account creation if needed
                                } else {
                                    Some(blackhole_account)
                                };
                                
                                state_changes.push(TronStateChange::AccountChange {
                                    address: blackhole_address,
                                    old_account: old_blackhole_account,
                                    new_account: Some(new_blackhole_account),
                                });
                                
                                debug!("Credited fee {} SUN to blackhole address {}", fee_amount, fee_config.blackhole_address_base58);
                            },
                            Err(e) => {
                                warn!("Invalid blackhole address '{}': {}, falling back to burn mode", 
                                      fee_config.blackhole_address_base58, e);
                            }
                        }
                    }
                },
                _ => {
                    debug!("Unknown fee mode '{}', defaulting to burn", fee_config.mode);
                }
            }
        } else {
            debug!("No fee to process (fee_amount = 0), skipping fee handling");
        }
        
        // Sort state changes deterministically for digest parity
        state_changes.sort_by(|a, b| {
            match (a, b) {
                (TronStateChange::AccountChange { address: addr_a, .. }, 
                 TronStateChange::AccountChange { address: addr_b, .. }) => {
                    addr_a.cmp(addr_b)
                },
                (TronStateChange::StorageChange { address: addr_a, key: key_a, .. },
                 TronStateChange::StorageChange { address: addr_b, key: key_b, .. }) => {
                    addr_a.cmp(addr_b).then(key_a.cmp(key_b))
                },
                // Account changes before storage changes for same address
                (TronStateChange::AccountChange { address: addr_a, .. },
                 TronStateChange::StorageChange { address: addr_b, .. }) => {
                    addr_a.cmp(addr_b).then(std::cmp::Ordering::Less)
                },
                (TronStateChange::StorageChange { address: addr_a, .. },
                 TronStateChange::AccountChange { address: addr_b, .. }) => {
                    addr_a.cmp(addr_b).then(std::cmp::Ordering::Greater)
                },
            }
        });
        
        debug!("Non-VM transaction executed successfully - bandwidth_used: {}, fee: {} SUN, state_changes: {}", 
               bandwidth_used, fee_amount, state_changes.len());
        
        Ok(TronExecutionResult {
            success: true,
            return_data: revm_primitives::Bytes::new(), // No return data for value transfers
            energy_used: 0, // Non-VM transactions use 0 energy
            bandwidth_used,
            state_changes,
            logs: Vec::new(), // No logs for value transfers
            error: None,
        })
    }
    
    /// Calculate bandwidth usage for a transaction based on its serialized size
    fn calculate_bandwidth_usage(transaction: &TronTransaction) -> u64 {
        // Approximate bandwidth calculation based on transaction fields
        // This is a simplified version - full implementation would consider exact protobuf serialization
        
        let base_size = 60; // Base transaction overhead (addresses, nonce, etc.)
        let data_size = transaction.data.len() as u64;
        let signature_size = 65; // ECDSA signature size
        
        base_size + data_size + signature_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tron_backend_execution::{TronTransaction, TronExecutionContext};
    use tron_backend_common::{ExecutionConfig, ExecutionFeeConfig};
    use revm_primitives::{Address, U256, Bytes};
    
    #[test]
    fn test_calculate_bandwidth_usage() {
        // Test basic transaction
        let tx = TronTransaction {
            from: Address::ZERO,
            to: Some(Address::ZERO),
            value: U256::from(100),
            data: Bytes::new(),
            gas_limit: 21000,
            gas_price: U256::ZERO,
            nonce: 0,
        };
        
        let bandwidth = BackendService::calculate_bandwidth_usage(&tx);
        assert_eq!(bandwidth, 60 + 0 + 65); // base_size + data_size + signature_size
        
        // Test transaction with data
        let tx_with_data = TronTransaction {
            from: Address::ZERO,
            to: Some(Address::ZERO),
            value: U256::from(100),
            data: Bytes::from(vec![0x60, 0x60, 0x60, 0x40]), // 4 bytes of data
            gas_limit: 21000,
            gas_price: U256::ZERO,
            nonce: 0,
        };
        
        let bandwidth_with_data = BackendService::calculate_bandwidth_usage(&tx_with_data);
        assert_eq!(bandwidth_with_data, 60 + 4 + 65); // base_size + data_size + signature_size
    }
    
    #[test]
    fn test_tx_kind_conversion() {
        // Test that TxKind enum values can be converted
        assert_eq!(crate::backend::TxKind::NonVm as i32, 0);
        assert_eq!(crate::backend::TxKind::Vm as i32, 1);
        
        // Test conversion from i32
        assert_eq!(crate::backend::TxKind::try_from(0).unwrap(), crate::backend::TxKind::NonVm);
        assert_eq!(crate::backend::TxKind::try_from(1).unwrap(), crate::backend::TxKind::Vm);
    }
    
    #[test] 
    fn test_address_conversion_helpers() {
        // Test Tron address prefix stripping
        let tron_address_with_prefix = vec![0x41, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78];
        let stripped = BackendService::strip_tron_address_prefix(&tron_address_with_prefix).unwrap();
        assert_eq!(stripped.len(), 20);
        assert_eq!(stripped[0], 0x12);
        
        let evm_address_no_prefix = vec![0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78];
        let already_stripped = BackendService::strip_tron_address_prefix(&evm_address_no_prefix).unwrap();
        assert_eq!(already_stripped.len(), 20);
        assert_eq!(already_stripped, &evm_address_no_prefix);
        
        // Test adding Tron address prefix
        let address = Address::from_slice(&[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0x56, 0x78]);
        let with_prefix = BackendService::add_tron_address_prefix(&address);
        assert_eq!(with_prefix.len(), 21);
        assert_eq!(with_prefix[0], 0x41);
        assert_eq!(&with_prefix[1..], address.as_slice());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use revm_primitives::{Address, U256, Bytes};
    
    // Mock storage adapter for testing
    struct MockStorageAdapter {
        accounts: Arc<RwLock<HashMap<Address, revm_primitives::AccountInfo>>>,
    }
    
    impl MockStorageAdapter {
        fn new() -> Self {
            Self {
                accounts: Arc::new(RwLock::new(HashMap::new())),
            }
        }
        
        async fn set_account(&self, address: Address, account: revm_primitives::AccountInfo) {
            self.accounts.write().await.insert(address, account);
        }
        
        async fn get_account(&self, address: &Address) -> Option<revm_primitives::AccountInfo> {
            self.accounts.read().await.get(address).cloned()
        }
    }
    
    // Note: These tests would require more setup to actually run, including mock storage adapters
    // They serve as examples of what could be tested in a full integration test suite
    
    #[tokio::test]
    #[ignore] // Ignored because it requires full system setup
    async fn test_non_vm_transaction_execution() {
        // This test would set up a full BackendService with mock storage
        // and test the complete non-VM transaction execution flow
        
        // Setup mock accounts
        let sender_address = Address::from_slice(&[0x01; 20]);
        let recipient_address = Address::from_slice(&[0x02; 20]);
        
        let sender_account = revm_primitives::AccountInfo {
            balance: U256::from(1000000u64), // 1M SUN
            nonce: 0,
            code_hash: revm_primitives::B256::ZERO,
            code: None,
        };
        
        let recipient_account = revm_primitives::AccountInfo::default();
        
        // Create transaction
        let transaction = TronTransaction {
            from: sender_address,
            to: Some(recipient_address),
            value: U256::from(100000u64), // 100K SUN transfer
            data: Bytes::new(), // No data = non-VM transaction
            gas_limit: 0, // Non-VM transactions don't use gas
            gas_price: U256::ZERO,
            nonce: 0,
        };
        
        let context = TronExecutionContext {
            block_number: 1000,
            block_timestamp: 1640000000,
            block_coinbase: Address::ZERO,
            block_difficulty: U256::from(1),
            block_gas_limit: 1000000,
            chain_id: 0x2b6653dc,
            energy_price: 420,
            bandwidth_price: 1000,
        };
        
        // Expected results:
        // - sender balance: 1000000 - 100000 - fee = 1000000 - 100000 - 125*1000 = 775000
        // - recipient balance: 0 + 100000 = 100000
        // - bandwidth_used: 60 + 0 + 65 = 125 bytes
        // - energy_used: 0 (non-VM)
        // - state_changes: 2 (sender + recipient) or 3 (if blackhole fee)
    }
    
    #[tokio::test] 
    #[ignore] // Ignored because it requires full system setup
    async fn test_fee_handling_modes() {
        // This test would verify different fee handling modes:
        // 1. "burn" mode - no additional state changes for fees
        // 2. "blackhole" mode - additional state change crediting blackhole account
        // 3. Invalid blackhole address handling
    }
}

#[tonic::async_trait]
impl crate::backend::backend_server::Backend for BackendService {
    type IteratorStream = std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<IteratorResponse, Status>> + Send>>;
    type StreamMetricsStream = std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<MetricsResponse, Status>> + Send>>;
    // Health and metadata
    async fn health(&self, _request: Request<HealthRequest>) -> Result<Response<HealthResponse>, Status> {
        debug!("Health check requested");
        
        let health_map = self.module_manager.health_all().await;
        let mut overall_status = health_response::Status::Healthy;
        let mut module_status = HashMap::new();
        
        for (module_name, health) in health_map {
            match health.status {
                HealthStatus::Healthy => {
                    module_status.insert(module_name, "healthy".to_string());
                }
                HealthStatus::Degraded => {
                    if overall_status == health_response::Status::Healthy {
                        overall_status = health_response::Status::Degraded;
                    }
                    module_status.insert(module_name, "degraded".to_string());
                }
                HealthStatus::Unhealthy => {
                    overall_status = health_response::Status::Unhealthy;
                    module_status.insert(module_name, "unhealthy".to_string());
                }
            };
        }
        
        let response = HealthResponse {
            status: overall_status as i32,
            message: "Backend health check".to_string(),
            module_status,
        };
        
        Ok(Response::new(response))
    }
    
    async fn get_metadata(&self, _request: Request<MetadataRequest>) -> Result<Response<MetadataResponse>, Status> {
        debug!("Metadata requested");
        
        let uptime = self.start_time.elapsed()
            .unwrap_or_default()
            .as_secs() as i64;
        
        let response = MetadataResponse {
            version: env!("CARGO_PKG_VERSION").to_string(),
            enabled_modules: self.module_manager.module_names(),
            module_versions: self.module_manager.module_versions(),
            uptime_seconds: uptime,
        };
        
        Ok(Response::new(response))
    }
    
    // Basic Storage Operations
    async fn get(&self, request: Request<GetRequest>) -> Result<Response<GetResponse>, Status> {
        debug!("Get request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.get(&req.database, &req.key) {
            Ok(Some(value)) => {
                let response = GetResponse {
                    value,
                    found: true,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Ok(None) => {
                let response = GetResponse {
                    value: Vec::new(),
                    found: false,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Get operation failed: {}", e);
                let response = GetResponse {
                    value: Vec::new(),
                    found: false,
                    success: false,
                    error_message: format!("Get operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn put(&self, request: Request<PutRequest>) -> Result<Response<PutResponse>, Status> {
        debug!("Put request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.put(&req.database, &req.key, &req.value) {
            Ok(()) => {
                let response = PutResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Put operation failed: {}", e);
                let response = PutResponse {
                    success: false,
                    error_message: format!("Put operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn delete(&self, request: Request<DeleteRequest>) -> Result<Response<DeleteResponse>, Status> {
        debug!("Delete request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.delete(&req.database, &req.key) {
            Ok(()) => {
                let response = DeleteResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Delete operation failed: {}", e);
                let response = DeleteResponse {
                    success: false,
                    error_message: format!("Delete operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn has(&self, request: Request<HasRequest>) -> Result<Response<HasResponse>, Status> {
        debug!("Has request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.has(&req.database, &req.key) {
            Ok(exists) => {
                let response = HasResponse {
                    exists,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Has operation failed: {}", e);
                let response = HasResponse {
                    exists: false,
                    success: false,
                    error_message: format!("Has operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    // Batch Storage Operations
    async fn batch_write(&self, request: Request<BatchWriteRequest>) -> Result<Response<BatchWriteResponse>, Status> {
        debug!("Batch write request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        // Convert protobuf operations to engine operations
        let operations: Vec<tron_backend_storage::WriteOperation> = req.operations
            .into_iter()
            .map(|op| tron_backend_storage::WriteOperation {
                r#type: op.r#type,
                key: op.key,
                value: op.value,
            })
            .collect();
        
        match engine.batch_write(&req.database, &operations) {
            Ok(()) => {
                let response = BatchWriteResponse {
                    success: true,
                    error_message: String::new(),
                    operations_applied: operations.len() as i32,
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Batch write operation failed: {}", e);
                let response = BatchWriteResponse {
                    success: false,
                    error_message: format!("Batch write operation failed: {}", e),
                    operations_applied: 0,
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn batch_get(&self, request: Request<BatchGetRequest>) -> Result<Response<BatchGetResponse>, Status> {
        debug!("Batch get request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.batch_get(&req.database, &req.keys) {
            Ok(results) => {
                let pairs: Vec<KeyValue> = results
                    .into_iter()
                    .map(|kv| KeyValue {
                        key: kv.key,
                        value: kv.value,
                        found: kv.found,
                    })
                    .collect();
                
                let response = BatchGetResponse {
                    pairs,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Batch get operation failed: {}", e);
                let response = BatchGetResponse {
                    pairs: Vec::new(),
                    success: false,
                    error_message: format!("Batch get operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    // Iterator Operations
    async fn iterator(&self, request: Request<IteratorRequest>) -> Result<Response<Self::IteratorStream>, Status> {
        debug!("Iterator request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        let (tx, rx) = mpsc::channel(100);
        let database = req.database.clone();
        let start_key = req.start_key.clone();
        let engine_clone = engine.clone();
        
        tokio::spawn(async move {
            // For simplicity, we'll get all keys and stream them
            // In a real implementation, you'd want to use RocksDB iterators more efficiently
            match engine_clone.get_next(&database, &start_key, 1000) {
                Ok(pairs) => {
                    for pair in pairs {
                        let response = IteratorResponse {
                            key: pair.key,
                            value: pair.value,
                            end_of_stream: false,
                        };
                        if tx.send(Ok(response)).await.is_err() {
                            break;
                        }
                    }
                    // Send end of stream marker
                    let _ = tx.send(Ok(IteratorResponse {
                        key: Vec::new(),
                        value: Vec::new(),
                        end_of_stream: true,
                    })).await;
                }
                Err(e) => {
                    let _ = tx.send(Err(Status::internal(format!("Iterator failed: {}", e)))).await;
                }
            }
        });
        
        let stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream) as Self::IteratorStream))
    }
    
    async fn get_keys_next(&self, request: Request<GetKeysNextRequest>) -> Result<Response<GetKeysNextResponse>, Status> {
        debug!("Get keys next request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.get_keys_next(&req.database, &req.start_key, req.limit) {
            Ok(keys) => {
                let response = GetKeysNextResponse {
                    keys,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Get keys next operation failed: {}", e);
                let response = GetKeysNextResponse {
                    keys: Vec::new(),
                    success: false,
                    error_message: format!("Get keys next operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn get_values_next(&self, request: Request<GetValuesNextRequest>) -> Result<Response<GetValuesNextResponse>, Status> {
        debug!("Get values next request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.get_values_next(&req.database, &req.start_key, req.limit) {
            Ok(values) => {
                let response = GetValuesNextResponse {
                    values,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Get values next operation failed: {}", e);
                let response = GetValuesNextResponse {
                    values: Vec::new(),
                    success: false,
                    error_message: format!("Get values next operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn get_next(&self, request: Request<GetNextRequest>) -> Result<Response<GetNextResponse>, Status> {
        debug!("Get next request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.get_next(&req.database, &req.start_key, req.limit) {
            Ok(results) => {
                let pairs: Vec<KeyValue> = results
                    .into_iter()
                    .map(|kv| KeyValue {
                        key: kv.key,
                        value: kv.value,
                        found: kv.found,
                    })
                    .collect();
                
                let response = GetNextResponse {
                    pairs,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Get next operation failed: {}", e);
                let response = GetNextResponse {
                    pairs: Vec::new(),
                    success: false,
                    error_message: format!("Get next operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn prefix_query(&self, request: Request<PrefixQueryRequest>) -> Result<Response<PrefixQueryResponse>, Status> {
        debug!("Prefix query request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.prefix_query(&req.database, &req.prefix) {
            Ok(results) => {
                let pairs: Vec<KeyValue> = results
                    .into_iter()
                    .map(|kv| KeyValue {
                        key: kv.key,
                        value: kv.value,
                        found: kv.found,
                    })
                    .collect();
                
                let response = PrefixQueryResponse {
                    pairs,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Prefix query operation failed: {}", e);
                let response = PrefixQueryResponse {
                    pairs: Vec::new(),
                    success: false,
                    error_message: format!("Prefix query operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    

    
    // Snapshot Support
    async fn create_snapshot(&self, request: Request<CreateSnapshotRequest>) -> Result<Response<CreateSnapshotResponse>, Status> {
        debug!("Create snapshot request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.create_snapshot(&req.database) {
            Ok(snapshot_id) => {
                let response = CreateSnapshotResponse {
                    snapshot_id,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Create snapshot operation failed: {}", e);
                let response = CreateSnapshotResponse {
                    snapshot_id: String::new(),
                    success: false,
                    error_message: format!("Create snapshot operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn delete_snapshot(&self, request: Request<DeleteSnapshotRequest>) -> Result<Response<DeleteSnapshotResponse>, Status> {
        debug!("Delete snapshot request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.delete_snapshot(&req.snapshot_id) {
            Ok(()) => {
                let response = DeleteSnapshotResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Delete snapshot operation failed: {}", e);
                let response = DeleteSnapshotResponse {
                    success: false,
                    error_message: format!("Delete snapshot operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn get_from_snapshot(&self, request: Request<GetFromSnapshotRequest>) -> Result<Response<GetFromSnapshotResponse>, Status> {
        debug!("Get from snapshot request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.get_from_snapshot(&req.snapshot_id, &req.key) {
            Ok(Some(value)) => {
                let response = GetFromSnapshotResponse {
                    value,
                    found: true,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Ok(None) => {
                let response = GetFromSnapshotResponse {
                    value: Vec::new(),
                    found: false,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Get from snapshot operation failed: {}", e);
                let response = GetFromSnapshotResponse {
                    value: Vec::new(),
                    found: false,
                    success: false,
                    error_message: format!("Get from snapshot operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    // Transaction Support
    async fn begin_transaction(&self, request: Request<BeginTransactionRequest>) -> Result<Response<BeginTransactionResponse>, Status> {
        debug!("Begin transaction request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.begin_transaction(&req.database) {
            Ok(transaction_id) => {
                let response = BeginTransactionResponse {
                    transaction_id,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Begin transaction operation failed: {}", e);
                let response = BeginTransactionResponse {
                    transaction_id: String::new(),
                    success: false,
                    error_message: format!("Begin transaction operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn commit_transaction(&self, request: Request<CommitTransactionRequest>) -> Result<Response<CommitTransactionResponse>, Status> {
        debug!("Commit transaction request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.commit_transaction(&req.transaction_id) {
            Ok(()) => {
                let response = CommitTransactionResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Commit transaction operation failed: {}", e);
                let response = CommitTransactionResponse {
                    success: false,
                    error_message: format!("Commit transaction operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn rollback_transaction(&self, request: Request<RollbackTransactionRequest>) -> Result<Response<RollbackTransactionResponse>, Status> {
        debug!("Rollback transaction request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.rollback_transaction(&req.transaction_id) {
            Ok(()) => {
                let response = RollbackTransactionResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Rollback transaction operation failed: {}", e);
                let response = RollbackTransactionResponse {
                    success: false,
                    error_message: format!("Rollback transaction operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    // Database Management Operations
    async fn init_db(&self, request: Request<InitDbRequest>) -> Result<Response<InitDbResponse>, Status> {
        debug!("Init DB request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        // Convert protobuf StorageConfig to engine StorageConfig
        let config = req.config.map(|c| tron_backend_storage::StorageConfig {
            engine: c.engine,
            engine_options: c.engine_options,
            enable_statistics: c.enable_statistics,
            max_open_files: c.max_open_files,
            block_cache_size: c.block_cache_size,
        }).unwrap_or_else(|| tron_backend_storage::StorageConfig {
            engine: "ROCKSDB".to_string(),
            engine_options: std::collections::HashMap::new(),
            enable_statistics: true,
            max_open_files: 1000,
            block_cache_size: 8 * 1024 * 1024,
        });
        
        match engine.init_db(&req.database, &config) {
            Ok(()) => {
                let response = InitDbResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Init DB operation failed: {}", e);
                let response = InitDbResponse {
                    success: false,
                    error_message: format!("Init DB operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn close_db(&self, request: Request<CloseDbRequest>) -> Result<Response<CloseDbResponse>, Status> {
        debug!("Close DB request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.close_db(&req.database) {
            Ok(()) => {
                let response = CloseDbResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Close DB operation failed: {}", e);
                let response = CloseDbResponse {
                    success: false,
                    error_message: format!("Close DB operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn reset_db(&self, request: Request<ResetDbRequest>) -> Result<Response<ResetDbResponse>, Status> {
        debug!("Reset DB request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.reset_db(&req.database) {
            Ok(()) => {
                let response = ResetDbResponse {
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Reset DB operation failed: {}", e);
                let response = ResetDbResponse {
                    success: false,
                    error_message: format!("Reset DB operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn is_alive(&self, request: Request<IsAliveRequest>) -> Result<Response<IsAliveResponse>, Status> {
        debug!("Is alive request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.is_alive(&req.database) {
            Ok(alive) => {
                let response = IsAliveResponse {
                    alive,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Is alive operation failed: {}", e);
                let response = IsAliveResponse {
                    alive: false,
                    success: false,
                    error_message: format!("Is alive operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn size(&self, request: Request<SizeRequest>) -> Result<Response<SizeResponse>, Status> {
        debug!("Size request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.size(&req.database) {
            Ok(size) => {
                let response = SizeResponse {
                    size,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Size operation failed: {}", e);
                let response = SizeResponse {
                    size: 0,
                    success: false,
                    error_message: format!("Size operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn is_empty(&self, request: Request<IsEmptyRequest>) -> Result<Response<IsEmptyResponse>, Status> {
        debug!("Is empty request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.is_empty(&req.database) {
            Ok(empty) => {
                let response = IsEmptyResponse {
                    empty,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Is empty operation failed: {}", e);
                let response = IsEmptyResponse {
                    empty: false,
                    success: false,
                    error_message: format!("Is empty operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    

    
    // Storage Metadata & Monitoring
    async fn list_databases(&self, _request: Request<ListDatabasesRequest>) -> Result<Response<ListDatabasesResponse>, Status> {
        debug!("List databases request");
        
        let engine = self.get_storage_engine()?;
        
        match engine.list_databases() {
            Ok(databases) => {
                let response = ListDatabasesResponse {
                    databases,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("List databases operation failed: {}", e);
                let response = ListDatabasesResponse {
                    databases: Vec::new(),
                    success: false,
                    error_message: format!("List databases operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn get_stats(&self, request: Request<GetStatsRequest>) -> Result<Response<GetStatsResponse>, Status> {
        debug!("Get stats request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        match engine.get_stats(&req.database) {
            Ok(stats) => {
                let proto_stats = StorageStats {
                    total_keys: stats.total_keys,
                    total_size: stats.total_size,
                    engine_stats: stats.engine_stats,
                    last_modified: stats.last_modified,
                };
                
                let response = GetStatsResponse {
                    stats: Some(proto_stats),
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Get stats operation failed: {}", e);
                let response = GetStatsResponse {
                    stats: None,
                    success: false,
                    error_message: format!("Get stats operation failed: {}", e),
                };
                Ok(Response::new(response))
            }
        }
    }
    
    async fn stream_metrics(&self, request: Request<StreamMetricsRequest>) -> Result<Response<Self::StreamMetricsStream>, Status> {
        debug!("Stream metrics request: {:?}", request.get_ref());
        
        let req = request.into_inner();
        let engine = self.get_storage_engine()?;
        
        let (tx, rx) = mpsc::channel(100);
        let database = req.database.clone();
        let engine_clone = engine.clone();
        
        tokio::spawn(async move {
            // Send metrics periodically
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            
            loop {
                interval.tick().await;
                
                if database.is_empty() {
                    // Stream metrics for all databases
                    if let Ok(databases) = engine_clone.list_databases() {
                        for db_name in databases {
                            if let Ok(stats) = engine_clone.get_stats(&db_name) {
                                let mut metrics = HashMap::new();
                                metrics.insert("total_keys".to_string(), stats.total_keys as f64);
                                metrics.insert("total_size".to_string(), stats.total_size as f64);
                                
                                let response = MetricsResponse {
                                    database: db_name,
                                    metrics,
                                    timestamp: chrono::Utc::now().timestamp(),
                                };
                                
                                if tx.send(Ok(response)).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                } else {
                    // Stream metrics for specific database
                    if let Ok(stats) = engine_clone.get_stats(&database) {
                        let mut metrics = HashMap::new();
                        metrics.insert("total_keys".to_string(), stats.total_keys as f64);
                        metrics.insert("total_size".to_string(), stats.total_size as f64);
                        
                        let response = MetricsResponse {
                            database: database.clone(),
                            metrics,
                            timestamp: chrono::Utc::now().timestamp(),
                        };
                        
                        if tx.send(Ok(response)).await.is_err() {
                            return;
                        }
                    }
                }
            }
        });
        
        let stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream) as Self::StreamMetricsStream))
    }
    
    async fn compact_range(&self, request: Request<CompactRangeRequest>) -> Result<Response<CompactRangeResponse>, Status> {
        debug!("Compact range request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = CompactRangeResponse {
            success: true,
            error_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn get_property(&self, request: Request<GetPropertyRequest>) -> Result<Response<GetPropertyResponse>, Status> {
        debug!("Get property request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = GetPropertyResponse {
            value: String::new(),
            found: false,
            success: false,
            error_message: "Not implemented".to_string(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn backup_database(&self, request: Request<BackupDatabaseRequest>) -> Result<Response<BackupDatabaseResponse>, Status> {
        debug!("Backup database request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = BackupDatabaseResponse {
            success: true,
            error_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn restore_database(&self, request: Request<RestoreDatabaseRequest>) -> Result<Response<RestoreDatabaseResponse>, Status> {
        debug!("Restore database request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = RestoreDatabaseResponse {
            success: true,
            error_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
    
    // Execution operations (delegated to execution module)
    async fn execute_transaction(&self, request: Request<ExecuteTransactionRequest>) -> Result<Response<ExecuteTransactionResponse>, Status> {
        debug!("Execute transaction request: {:?}", request.get_ref());
        
        let req = request.get_ref();
        
        // Get the execution module
        let execution_module = self.get_execution_module()?;
        
        // Downcast to the concrete execution module type
        let execution_module = execution_module
            .as_any()
            .downcast_ref::<ExecutionModule>()
            .ok_or_else(|| Status::internal("Failed to downcast execution module"))?;
        
        // Convert protobuf types to execution types
        let (transaction, tx_kind) = match self.convert_protobuf_transaction(req.transaction.as_ref()) {
            Ok((tx, kind)) => {
                debug!("Converted transaction - gas_limit: {}, gas_price: {}, data_len: {}, kind: {:?}", 
                       tx.gas_limit, tx.gas_price, tx.data.len(), kind);
                (tx, kind)
            },
            Err(e) => {
                error!("Failed to convert transaction: {}", e);
                return Ok(Response::new(ExecuteTransactionResponse {
                    result: Some(ExecutionResult {
                        status: execution_result::Status::TronSpecificError as i32,
                        return_data: vec![],
                        energy_used: 0,
                        energy_refunded: 0,
                        state_changes: vec![],
                        logs: vec![],
                        error_message: format!("Transaction conversion error: {}", e),
                        bandwidth_used: 0,
                        resource_usage: vec![],
                    }),
                    success: false,
                    error_message: format!("Transaction conversion error: {}", e),
                }));
            }
        };
        
        let context = match self.convert_protobuf_context(req.context.as_ref()) {
            Ok(ctx) => {
                debug!("Converted context - block_gas_limit: {}, energy_price: {}", 
                       ctx.block_gas_limit, ctx.energy_price);
                ctx
            },
            Err(e) => {
                error!("Failed to convert execution context: {}", e);
                return Ok(Response::new(ExecuteTransactionResponse {
                    result: Some(ExecutionResult {
                        status: execution_result::Status::TronSpecificError as i32,
                        return_data: vec![],
                        energy_used: 0,
                        energy_refunded: 0,
                        state_changes: vec![],
                        logs: vec![],
                        error_message: format!("Context conversion error: {}", e),
                        bandwidth_used: 0,
                        resource_usage: vec![],
                    }),
                    success: false,
                    error_message: format!("Context conversion error: {}", e),
                }));
            }
        };
        
        // Get the storage engine and create a unified storage adapter
        let storage_engine = self.get_storage_engine()?;
        let storage_adapter = tron_backend_execution::StorageModuleAdapter::new(
            storage_engine.clone(),
        );

        // Phase 3: Branch execution based on transaction kind
        let execution_result = match tx_kind {
            crate::backend::TxKind::NonVm => {
                info!("Executing NON_VM transaction natively (bypassing EVM)");
                // Execute non-VM transaction natively without EVM
                match self.execute_non_vm_transaction(&storage_adapter, &transaction, &context) {
                    Ok(result) => {
                        info!("Non-VM transaction executed successfully - energy_used: {}, bandwidth_used: {}, state_changes: {}",
                              result.energy_used, result.bandwidth_used, result.state_changes.len());
                        Ok(result)
                    },
                    Err(e) => {
                        error!("Non-VM transaction execution failed: {}", e);
                        Err(anyhow::anyhow!("Non-VM execution error: {}", e))
                    }
                }
            },
            crate::backend::TxKind::Vm => {
                info!("Executing VM transaction via EVM");
                
                // TRON Parity Fix: Check if this is likely a non-VM transaction before execution (fallback heuristic)
                let is_non_vm = self.is_likely_non_vm_transaction(&transaction, &storage_adapter);
                
                // Execute the transaction using the database-specific storage adapter
                match execution_module.execute_transaction_with_storage(storage_adapter, &transaction, &context) {
                    Ok(mut result) => {
                        // TRON Parity Fix: Apply non-VM heuristic to set energy_used = 0 for non-VM transactions
                        if is_non_vm {
                            debug!("Detected likely non-VM transaction (empty data, no code at 'to' address) - setting energy_used = 0");
                            debug!("Original energy_used: {}, from: {:?}, to: {:?}, value: {}", 
                                   result.energy_used, transaction.from, transaction.to, transaction.value);
                            result.energy_used = 0;
                        } else {
                            debug!("Detected VM transaction - keeping original energy_used: {}", result.energy_used);
                        }
                        
                        // TRON Phase 2: Apply fee post-processing based on configuration
                        if let Err(e) = self.apply_fee_post_processing(&mut result, &transaction, &context, is_non_vm) {
                            warn!("Fee post-processing failed: {}, continuing with original result", e);
                            // Continue with original result
                        }
                        
                        Ok(result)
                    },
                    Err(e) => Err(e)
                }
            }
        };

        // Handle execution result
        match execution_result {
            Ok(result) => {
                info!("Transaction executed successfully - energy_used: {}, bandwidth_used: {}", 
                      result.energy_used, result.bandwidth_used);
                let response = self.convert_execution_result_to_protobuf(result);
                Ok(Response::new(response))
            }
            Err(e) => {
                let error_str = format!("{}", e);
                error!("Transaction execution failed: {}", error_str);
                
                // Check if it's a gas-related error
                if error_str.contains("CallGasCostMoreThanGasLimit") || error_str.contains("OutOfGas") {
                    warn!("Gas limit issue detected - tx.gas_limit: {}, block.gas_limit: {}", 
                          transaction.gas_limit, context.block_gas_limit);
                }
                
                Ok(Response::new(ExecuteTransactionResponse {
                    result: Some(ExecutionResult {
                        status: execution_result::Status::TronSpecificError as i32,
                        return_data: vec![],
                        energy_used: 0,
                        energy_refunded: 0,
                        state_changes: vec![],
                        logs: vec![],
                        error_message: format!("Execution error: {}", e),
                        bandwidth_used: 0,
                        resource_usage: vec![],
                    }),
                    success: false,
                    error_message: format!("Execution error: {}", e),
                }))
            }
        }
    }
    
    async fn call_contract(&self, request: Request<CallContractRequest>) -> Result<Response<CallContractResponse>, Status> {
        debug!("Call contract request: {:?}", request.get_ref());

        let req = request.get_ref();

        // Get the execution module
        let execution_module = self.get_execution_module()?;

        // Downcast to the concrete execution module type
        let execution_module = execution_module
            .as_any()
            .downcast_ref::<ExecutionModule>()
            .ok_or_else(|| Status::internal("Failed to downcast execution module"))?;

        // Convert protobuf types to execution types
        let transaction = match self.convert_call_contract_request_to_transaction(req) {
            Ok(tx) => tx,
            Err(e) => {
                error!("Failed to convert call contract request: {}", e);
                return Ok(Response::new(CallContractResponse {
                    return_data: vec![],
                    success: false,
                    error_message: format!("Request conversion error: {}", e),
                    energy_used: 0,
                }));
            }
        };

        let context = match self.convert_protobuf_context(req.context.as_ref()) {
            Ok(ctx) => ctx,
            Err(e) => {
                error!("Failed to convert execution context: {}", e);
                return Ok(Response::new(CallContractResponse {
                    return_data: vec![],
                    success: false,
                    error_message: format!("Context conversion error: {}", e),
                    energy_used: 0,
                }));
            }
        };

        // Get the storage engine and create a unified storage adapter
        let storage_engine = self.get_storage_engine()?;
        let storage_adapter = tron_backend_execution::StorageModuleAdapter::new(
            storage_engine.clone(),
        );

        // Call the contract using the database-specific storage adapter
        match execution_module.call_contract_with_storage(storage_adapter, &transaction, &context) {
            Ok(result) => {
                let response = CallContractResponse {
                    return_data: result.return_data.to_vec(),
                    success: true,
                    error_message: String::new(),
                    energy_used: result.energy_used as i64,
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Contract call failed: {}", e);
                Ok(Response::new(CallContractResponse {
                    return_data: vec![],
                    success: false,
                    error_message: format!("Contract call error: {}", e),
                    energy_used: 0,
                }))
            }
        }
    }
    
    async fn estimate_energy(&self, request: Request<EstimateEnergyRequest>) -> Result<Response<EstimateEnergyResponse>, Status> {
        debug!("Estimate energy request: {:?}", request.get_ref());

        let req = request.get_ref();

        // Get the execution module
        let execution_module = self.get_execution_module()?;

        // Downcast to the concrete execution module type
        let execution_module = execution_module
            .as_any()
            .downcast_ref::<ExecutionModule>()
            .ok_or_else(|| Status::internal("Failed to downcast execution module"))?;

        // Convert protobuf types to execution types
        let transaction = match self.convert_protobuf_transaction(req.transaction.as_ref()) {
            Ok(tx) => tx,
            Err(e) => {
                error!("Failed to convert transaction: {}", e);
                return Ok(Response::new(EstimateEnergyResponse {
                    energy_estimate: 21000, // Default estimate on error
                    success: false,
                    error_message: format!("Transaction conversion error: {}", e),
                }));
            }
        };

        let context = match self.convert_protobuf_context(req.context.as_ref()) {
            Ok(ctx) => ctx,
            Err(e) => {
                error!("Failed to convert execution context: {}", e);
                return Ok(Response::new(EstimateEnergyResponse {
                    energy_estimate: 21000, // Default estimate on error
                    success: false,
                    error_message: format!("Context conversion error: {}", e),
                }));
            }
        };

        // Get the storage engine and create a unified storage adapter
        let storage_engine = self.get_storage_engine()?;
        let storage_adapter = tron_backend_execution::StorageModuleAdapter::new(
            storage_engine.clone(),
        );

        // Estimate energy using the database-specific storage adapter
        // Convert protobuf types to execution types (for estimate_energy, we don't need tx_kind)
        let (transaction_only, _) = match self.convert_protobuf_transaction(req.transaction.as_ref()) {
            Ok((tx, _kind)) => (tx, _kind),
            Err(e) => {
                error!("Failed to convert transaction: {}", e);
                return Ok(Response::new(EstimateEnergyResponse {
                    energy_estimate: 21000, // Default estimate on error
                    success: false,
                    error_message: format!("Transaction conversion error: {}", e),
                }));
            }
        };
        
        match execution_module.estimate_energy_with_storage(storage_adapter, &transaction_only, &context) {
            Ok(estimate) => {
                let response = EstimateEnergyResponse {
                    energy_estimate: estimate as i64,
                    success: true,
                    error_message: String::new(),
                };
                Ok(Response::new(response))
            }
            Err(e) => {
                error!("Energy estimation failed: {}", e);
                Ok(Response::new(EstimateEnergyResponse {
                    energy_estimate: 21000, // Default estimate on error
                    success: false,
                    error_message: format!("Energy estimation error: {}", e),
                }))
            }
        }
    }
    
    async fn get_code(&self, request: Request<GetCodeRequest>) -> Result<Response<GetCodeResponse>, Status> {
        debug!("Get code request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = GetCodeResponse {
            code: vec![],
            found: false,
            success: false,
            error_message: "Not implemented".to_string(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn get_storage_at(&self, request: Request<GetStorageAtRequest>) -> Result<Response<GetStorageAtResponse>, Status> {
        debug!("Get storage at request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = GetStorageAtResponse {
            value: vec![],
            found: false,
            success: false,
            error_message: "Not implemented".to_string(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn get_nonce(&self, request: Request<GetNonceRequest>) -> Result<Response<GetNonceResponse>, Status> {
        debug!("Get nonce request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = GetNonceResponse {
            nonce: 0,
            found: false,
            success: false,
            error_message: "Not implemented".to_string(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn get_balance(&self, request: Request<GetBalanceRequest>) -> Result<Response<GetBalanceResponse>, Status> {
        debug!("Get balance request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = GetBalanceResponse {
            balance: vec![],
            found: false,
            success: false,
            error_message: "Not implemented".to_string(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn create_evm_snapshot(&self, request: Request<CreateEvmSnapshotRequest>) -> Result<Response<CreateEvmSnapshotResponse>, Status> {
        debug!("Create EVM snapshot request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = CreateEvmSnapshotResponse {
            snapshot_id: uuid::Uuid::new_v4().to_string(),
            success: true,
            error_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
    
    async fn revert_to_evm_snapshot(&self, request: Request<RevertToEvmSnapshotRequest>) -> Result<Response<RevertToEvmSnapshotResponse>, Status> {
        debug!("Revert to EVM snapshot request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = RevertToEvmSnapshotResponse {
            success: true,
            error_message: String::new(),
        };
        
        Ok(Response::new(response))
    }
}

impl BackendService {
    // Helper functions for Tron address format conversion
    fn strip_tron_address_prefix(address_bytes: &[u8]) -> Result<&[u8], String> {
        if address_bytes.len() == 21 && address_bytes[0] == 0x41 {
            Ok(&address_bytes[1..]) // Skip the 0x41 prefix, return 20 bytes
        } else if address_bytes.len() == 20 {
            Ok(address_bytes) // Already 20 bytes, no prefix
        } else {
            Err(format!("Invalid address length: expected 20 or 21 bytes (with 0x41 prefix), got {}", address_bytes.len()))
        }
    }
    
    fn add_tron_address_prefix(address: &revm_primitives::Address) -> Vec<u8> {
        let mut result = Vec::with_capacity(21);
        result.push(0x41); // Add Tron address prefix
        result.extend_from_slice(address.as_slice());
        result
    }
    
    // Helper functions for converting between protobuf and execution types
    fn convert_protobuf_transaction(&self, tx: Option<&crate::backend::TronTransaction>) -> Result<(TronTransaction, crate::backend::TxKind), String> {
        let tx = tx.ok_or("Transaction is required")?;
        
        // Log the raw transaction data from Java
        debug!("Raw transaction from Java - energy_limit: {}, energy_price: {}, data_len: {}", 
               tx.energy_limit, tx.energy_price, tx.data.len());
        
        // Convert bytes to Address (strip Tron 0x41 prefix if present)
        let from_bytes = Self::strip_tron_address_prefix(&tx.from)?;
        let from = revm_primitives::Address::from_slice(from_bytes);
        
        let to = if tx.to.is_empty() {
            None // Contract creation
        } else {
            let to_bytes = Self::strip_tron_address_prefix(&tx.to)?;
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
        
        let transaction = TronTransaction {
            from,
            to,
            value,
            data: revm_primitives::Bytes::from(tx.data.clone()),
            gas_limit,
            gas_price,
            nonce: tx.nonce as u64,
        };
        
        // Extract tx_kind from protobuf, default to VM for backward compatibility
        let tx_kind = crate::backend::TxKind::try_from(tx.tx_kind).unwrap_or(crate::backend::TxKind::Vm);
        debug!("Transaction kind: {:?}", tx_kind);
        
        Ok((transaction, tx_kind))
    }
    
    fn convert_protobuf_context(&self, ctx: Option<&crate::backend::ExecutionContext>) -> Result<TronExecutionContext, String> {
        let ctx = ctx.ok_or("Execution context is required")?;
        
        // Log the raw context data from Java
        debug!("Raw context from Java - energy_limit: {}, energy_price: {}", 
               ctx.energy_limit, ctx.energy_price);
        
        // Strip Tron 0x41 prefix from coinbase address if present
        let coinbase_bytes = Self::strip_tron_address_prefix(&ctx.coinbase)?;
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

    fn convert_call_contract_request_to_transaction(&self, req: &crate::backend::CallContractRequest) -> Result<TronTransaction, String> {
        // Convert bytes to Address (strip Tron 0x41 prefix if present)
        let from_bytes = Self::strip_tron_address_prefix(&req.from)?;
        let from = revm_primitives::Address::from_slice(from_bytes);

        let to_bytes = Self::strip_tron_address_prefix(&req.to)?;
        let to = revm_primitives::Address::from_slice(to_bytes);

        Ok(TronTransaction {
            from,
            to: Some(to),
            value: revm_primitives::U256::ZERO, // Contract calls typically don't transfer value
            data: req.data.clone().into(),
            gas_limit: 1000000, // Default gas limit for contract calls
            gas_price: revm_primitives::U256::from(1),
            nonce: 0, // Contract calls don't use nonce
        })
    }

    fn convert_execution_result_to_protobuf(&self, result: TronExecutionResult) -> ExecuteTransactionResponse {
        let status = if result.success {
            execution_result::Status::Success
        } else {
            execution_result::Status::Revert
        };
        
        let logs: Vec<LogEntry> = result.logs.iter().map(|log| {
            LogEntry {
                address: Self::add_tron_address_prefix(&log.address),
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
                                address: Self::add_tron_address_prefix(address),
                                key: key.to_be_bytes::<32>().to_vec(),
                                old_value: old_value.to_be_bytes::<32>().to_vec(),
                                new_value: new_value.to_be_bytes::<32>().to_vec(),
                            }
                        ))
                    }
                },
                TronStateChange::AccountChange { address, old_account, new_account } => {
                    // Helper function to convert AccountInfo to protobuf
                    let convert_account_info = |addr: &revm::primitives::Address, acc_info: &revm::primitives::AccountInfo| {
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

                        crate::backend::AccountInfo {
                            address: Self::add_tron_address_prefix(addr),
                            balance: acc_info.balance.to_be_bytes::<32>().to_vec(),
                            nonce: acc_info.nonce,
                            code_hash: code_hash_bytes,
                            code: code_bytes,
                        }
                    };

                    let old_account_proto = old_account.as_ref().map(|acc| convert_account_info(address, acc));
                    let new_account_proto = new_account.as_ref().map(|acc| convert_account_info(address, acc));
                    
                    StateChange {
                        change: Some(crate::backend::state_change::Change::AccountChange(
                            crate::backend::AccountChange {
                                address: Self::add_tron_address_prefix(address),
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
            }),
            success: result.success,
            error_message,
        }
    }
} 

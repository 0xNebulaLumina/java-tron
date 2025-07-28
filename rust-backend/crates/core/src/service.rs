use std::collections::HashMap;
use std::time::SystemTime;

use tonic::{Request, Response, Status};
use tracing::{error, debug};
use tokio_stream::wrappers::ReceiverStream;
use tokio::sync::mpsc;

use tron_backend_common::{ModuleManager, HealthStatus};
use crate::backend::*;

// Import execution types
use tron_backend_execution::{
    TronTransaction, TronExecutionContext, TronExecutionResult,
    StorageEngineAdapter
};

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

    /// Convert protobuf TronTransaction to internal TronTransaction
    fn convert_transaction(&self, proto_tx: &crate::backend::TronTransaction) -> Result<TronTransaction, Status> {
        use revm::primitives::{Address, U256, Bytes};

        // Convert address from bytes
        let from = if proto_tx.from.len() == 20 {
            Address::from_slice(&proto_tx.from)
        } else {
            return Err(Status::invalid_argument("Invalid from address length"));
        };

        let to = if proto_tx.to.is_empty() {
            None
        } else if proto_tx.to.len() == 20 {
            Some(Address::from_slice(&proto_tx.to))
        } else {
            return Err(Status::invalid_argument("Invalid to address length"));
        };

        // Convert value from bytes to U256
        let value = if proto_tx.value.len() <= 32 {
            let mut value_bytes = [0u8; 32];
            let start = 32 - proto_tx.value.len();
            value_bytes[start..].copy_from_slice(&proto_tx.value);
            U256::from_be_bytes(value_bytes)
        } else {
            return Err(Status::invalid_argument("Value too large"));
        };

        let data = Bytes::from(proto_tx.data.clone());
        let gas_limit = proto_tx.energy_limit as u64;
        let gas_price = U256::from(proto_tx.energy_price as u64);
        let nonce = proto_tx.nonce as u64;

        Ok(TronTransaction {
            from,
            to,
            value,
            data,
            gas_limit,
            gas_price,
            nonce,
        })
    }

    /// Convert protobuf ExecutionContext to internal TronExecutionContext
    fn convert_context(&self, proto_ctx: &crate::backend::ExecutionContext) -> Result<TronExecutionContext, Status> {
        use revm::primitives::{Address, U256};

        let block_coinbase = if proto_ctx.coinbase.len() == 20 {
            Address::from_slice(&proto_ctx.coinbase)
        } else {
            Address::ZERO // Default coinbase
        };

        Ok(TronExecutionContext {
            block_number: proto_ctx.block_number as u64,
            block_timestamp: proto_ctx.block_timestamp as u64,
            block_coinbase,
            block_difficulty: U256::from(1), // Tron doesn't use PoW difficulty
            block_gas_limit: proto_ctx.energy_limit as u64,
            chain_id: 0x2b6653dc, // Tron mainnet chain ID
            energy_price: proto_ctx.energy_price as u64,
            bandwidth_price: 1000, // Default bandwidth price
        })
    }

    /// Convert internal TronExecutionResult to protobuf ExecutionResult
    fn convert_result(&self, result: &TronExecutionResult) -> crate::backend::ExecutionResult {
        use crate::backend::{execution_result, LogEntry, StateChange, TronResourceUsage};
        use crate::backend::tron_resource::Type as ResourceType;

        let status = if result.success {
            execution_result::Status::Success
        } else {
            execution_result::Status::Revert
        };

        // Convert logs
        let logs: Vec<LogEntry> = result.logs.iter().map(|log| {
            LogEntry {
                address: log.address.as_slice().to_vec(),
                topics: log.topics().iter().map(|topic| topic.as_slice().to_vec()).collect(),
                data: log.data.data.to_vec(),
            }
        }).collect();

        // Convert state changes from execution result
        let state_changes: Vec<StateChange> = result.state_changes.iter().map(|change| {
            self.convert_state_change(change)
        }).collect();

        // Create resource usage info
        let resource_usage = vec![
            TronResourceUsage {
                r#type: ResourceType::Energy as i32,
                used: result.energy_used as i64,
                total: 0, // Would need to be passed from context
                token_id: vec![],
            },
            TronResourceUsage {
                r#type: ResourceType::Bandwidth as i32,
                used: result.bandwidth_used as i64,
                total: 0, // Would need to be passed from context
                token_id: vec![],
            },
        ];

        crate::backend::ExecutionResult {
            status: status as i32,
            return_data: result.return_data.to_vec(),
            energy_used: result.energy_used as i64,
            energy_refunded: 0, // Not currently tracked
            state_changes,
            logs,
            error_message: result.error.clone().unwrap_or_default(),
            bandwidth_used: result.bandwidth_used as i64,
            resource_usage,
        }
    }

    /// Convert internal StateChange to protobuf StateChange
    fn convert_state_change(&self, change: &tron_backend_execution::StateChange) -> crate::backend::StateChange {
        use tron_backend_execution::StateChangeType;

        match &change.change_type {
            StateChangeType::AccountBalance { old_value, new_value } => {
                crate::backend::StateChange {
                    address: change.address.as_slice().to_vec(),
                    key: b"balance".to_vec(), // Special key for balance
                    old_value: old_value.to_be_bytes::<32>().to_vec(),
                    new_value: new_value.to_be_bytes::<32>().to_vec(),
                }
            }
            StateChangeType::AccountNonce { old_value, new_value } => {
                crate::backend::StateChange {
                    address: change.address.as_slice().to_vec(),
                    key: b"nonce".to_vec(), // Special key for nonce
                    old_value: old_value.to_be_bytes().to_vec(),
                    new_value: new_value.to_be_bytes().to_vec(),
                }
            }
            StateChangeType::AccountCode { old_value, new_value } => {
                crate::backend::StateChange {
                    address: change.address.as_slice().to_vec(),
                    key: b"code".to_vec(), // Special key for code
                    old_value: old_value.as_ref().map(|c| c.to_vec()).unwrap_or_default(),
                    new_value: new_value.as_ref().map(|c| c.to_vec()).unwrap_or_default(),
                }
            }
            StateChangeType::StorageSlot { key, old_value, new_value } => {
                crate::backend::StateChange {
                    address: change.address.as_slice().to_vec(),
                    key: key.to_be_bytes::<32>().to_vec(),
                    old_value: old_value.to_be_bytes::<32>().to_vec(),
                    new_value: new_value.to_be_bytes::<32>().to_vec(),
                }
            }
            StateChangeType::AccountCreated => {
                crate::backend::StateChange {
                    address: change.address.as_slice().to_vec(),
                    key: b"created".to_vec(), // Special key for account creation
                    old_value: vec![],
                    new_value: vec![1], // Indicate creation
                }
            }
            StateChangeType::AccountDeleted => {
                crate::backend::StateChange {
                    address: change.address.as_slice().to_vec(),
                    key: b"deleted".to_vec(), // Special key for account deletion
                    old_value: vec![1], // Indicate existence
                    new_value: vec![], // Indicate deletion
                }
            }
        }
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

        let req = request.into_inner();

        // Validate request
        if req.transaction.is_none() {
            return Err(Status::invalid_argument("Transaction is required"));
        }

        if req.context.is_none() {
            return Err(Status::invalid_argument("Execution context is required"));
        }

        let proto_tx = req.transaction.unwrap();
        let proto_ctx = req.context.unwrap();

        // Convert protobuf types to internal types
        let transaction = match self.convert_transaction(&proto_tx) {
            Ok(tx) => tx,
            Err(e) => {
                error!("Failed to convert transaction: {}", e);
                return Err(e);
            }
        };

        let context = match self.convert_context(&proto_ctx) {
            Ok(ctx) => ctx,
            Err(e) => {
                error!("Failed to convert context: {}", e);
                return Err(e);
            }
        };

        // Get execution module
        let execution_module = self.get_execution_module()?;
        let execution_module = execution_module
            .as_any()
            .downcast_ref::<tron_backend_execution::ExecutionModule>()
            .ok_or_else(|| Status::internal("Failed to downcast execution module"))?;

        // Get storage engine for the storage adapter
        let storage_engine = self.get_storage_engine()?;

        // Create storage adapter that uses the actual storage engine
        let storage_adapter = StorageEngineAdapter::new(storage_engine.clone(), req.database.clone());

        // Execute the transaction
        let execution_result = match execution_module.execute_transaction_with_storage(storage_adapter, &transaction, &context) {
            Ok(result) => result,
            Err(e) => {
                error!("Transaction execution failed: {}", e);
                return Ok(Response::new(ExecuteTransactionResponse {
                    result: Some(ExecutionResult {
                        status: execution_result::Status::Revert as i32,
                        return_data: vec![],
                        energy_used: 0,
                        energy_refunded: 0,
                        state_changes: vec![],
                        logs: vec![],
                        error_message: format!("Execution failed: {}", e),
                        bandwidth_used: 0,
                        resource_usage: vec![],
                    }),
                    success: false,
                    error_message: format!("Transaction execution failed: {}", e),
                }));
            }
        };

        // Convert result back to protobuf
        let proto_result = self.convert_result(&execution_result);

        let response = ExecuteTransactionResponse {
            result: Some(proto_result),
            success: execution_result.success,
            error_message: execution_result.error.unwrap_or_default(),
        };

        debug!("Transaction execution completed successfully");
        Ok(Response::new(response))
    }
    
    async fn call_contract(&self, request: Request<CallContractRequest>) -> Result<Response<CallContractResponse>, Status> {
        debug!("Call contract request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = CallContractResponse {
            return_data: vec![],
            success: true,
            error_message: String::new(),
            energy_used: 0,
        };
        
        Ok(Response::new(response))
    }
    
    async fn estimate_energy(&self, request: Request<EstimateEnergyRequest>) -> Result<Response<EstimateEnergyResponse>, Status> {
        debug!("Estimate energy request: {:?}", request.get_ref());
        
        // Placeholder implementation
        let response = EstimateEnergyResponse {
            energy_estimate: 21000, // Basic transaction cost
            success: true,
            error_message: String::new(),
        };
        
        Ok(Response::new(response))
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
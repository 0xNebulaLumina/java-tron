// gRPC implementation for BackendService
// This module contains the tonic gRPC trait implementation

pub mod address;
pub mod aext;
pub mod conversion;

use self::aext::parse_pre_execution_aext;
use self::conversion::*;
use super::BackendService;
use crate::backend::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};
use tron_backend_common::{to_tron_address, HealthStatus};
use tron_backend_execution::{
    EvmStateStore, ExecutionModule, ExecutionWriteBuffer, TouchedKey, TronExecutionContext,
    TronTransaction,
};
#[tonic::async_trait]
impl crate::backend::backend_server::Backend for BackendService {
    type IteratorStream = std::pin::Pin<
        Box<dyn tokio_stream::Stream<Item = Result<IteratorResponse, Status>> + Send>,
    >;
    type StreamMetricsStream =
        std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<MetricsResponse, Status>> + Send>>;
    // Health and metadata
    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
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

    async fn get_metadata(
        &self,
        _request: Request<MetadataRequest>,
    ) -> Result<Response<MetadataResponse>, Status> {
        debug!("Metadata requested");

        let uptime = self.start_time.elapsed().unwrap_or_default().as_secs() as i64;

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

    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
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
    async fn batch_write(
        &self,
        request: Request<BatchWriteRequest>,
    ) -> Result<Response<BatchWriteResponse>, Status> {
        debug!("Batch write request: {:?}", request.get_ref());

        let req = request.into_inner();
        let engine = self.get_storage_engine()?;

        // Convert protobuf operations to engine operations
        let operations: Vec<tron_backend_storage::WriteOperation> = req
            .operations
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

    async fn batch_get(
        &self,
        request: Request<BatchGetRequest>,
    ) -> Result<Response<BatchGetResponse>, Status> {
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
    async fn iterator(
        &self,
        request: Request<IteratorRequest>,
    ) -> Result<Response<Self::IteratorStream>, Status> {
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
                    let _ = tx
                        .send(Ok(IteratorResponse {
                            key: Vec::new(),
                            value: Vec::new(),
                            end_of_stream: true,
                        }))
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(Err(Status::internal(format!("Iterator failed: {}", e))))
                        .await;
                }
            }
        });

        let stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream) as Self::IteratorStream))
    }

    async fn get_keys_next(
        &self,
        request: Request<GetKeysNextRequest>,
    ) -> Result<Response<GetKeysNextResponse>, Status> {
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

    async fn get_values_next(
        &self,
        request: Request<GetValuesNextRequest>,
    ) -> Result<Response<GetValuesNextResponse>, Status> {
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

    async fn get_next(
        &self,
        request: Request<GetNextRequest>,
    ) -> Result<Response<GetNextResponse>, Status> {
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

    async fn prefix_query(
        &self,
        request: Request<PrefixQueryRequest>,
    ) -> Result<Response<PrefixQueryResponse>, Status> {
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
    async fn create_snapshot(
        &self,
        request: Request<CreateSnapshotRequest>,
    ) -> Result<Response<CreateSnapshotResponse>, Status> {
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

    async fn delete_snapshot(
        &self,
        request: Request<DeleteSnapshotRequest>,
    ) -> Result<Response<DeleteSnapshotResponse>, Status> {
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

    async fn get_from_snapshot(
        &self,
        request: Request<GetFromSnapshotRequest>,
    ) -> Result<Response<GetFromSnapshotResponse>, Status> {
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
    async fn begin_transaction(
        &self,
        request: Request<BeginTransactionRequest>,
    ) -> Result<Response<BeginTransactionResponse>, Status> {
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

    async fn commit_transaction(
        &self,
        request: Request<CommitTransactionRequest>,
    ) -> Result<Response<CommitTransactionResponse>, Status> {
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

    async fn rollback_transaction(
        &self,
        request: Request<RollbackTransactionRequest>,
    ) -> Result<Response<RollbackTransactionResponse>, Status> {
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
    async fn init_db(
        &self,
        request: Request<InitDbRequest>,
    ) -> Result<Response<InitDbResponse>, Status> {
        debug!("Init DB request: {:?}", request.get_ref());

        let req = request.into_inner();
        let engine = self.get_storage_engine()?;

        // Convert protobuf StorageConfig to engine StorageConfig
        let config = req
            .config
            .map(|c| tron_backend_storage::StorageConfig {
                engine: c.engine,
                engine_options: c.engine_options,
                enable_statistics: c.enable_statistics,
                max_open_files: c.max_open_files,
                block_cache_size: c.block_cache_size,
            })
            .unwrap_or_else(|| tron_backend_storage::StorageConfig {
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

    async fn close_db(
        &self,
        request: Request<CloseDbRequest>,
    ) -> Result<Response<CloseDbResponse>, Status> {
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

    async fn reset_db(
        &self,
        request: Request<ResetDbRequest>,
    ) -> Result<Response<ResetDbResponse>, Status> {
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

    async fn is_alive(
        &self,
        request: Request<IsAliveRequest>,
    ) -> Result<Response<IsAliveResponse>, Status> {
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

    async fn is_empty(
        &self,
        request: Request<IsEmptyRequest>,
    ) -> Result<Response<IsEmptyResponse>, Status> {
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
    async fn list_databases(
        &self,
        _request: Request<ListDatabasesRequest>,
    ) -> Result<Response<ListDatabasesResponse>, Status> {
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

    async fn get_stats(
        &self,
        request: Request<GetStatsRequest>,
    ) -> Result<Response<GetStatsResponse>, Status> {
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

    async fn stream_metrics(
        &self,
        request: Request<StreamMetricsRequest>,
    ) -> Result<Response<Self::StreamMetricsStream>, Status> {
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

    async fn compact_range(
        &self,
        request: Request<CompactRangeRequest>,
    ) -> Result<Response<CompactRangeResponse>, Status> {
        debug!("Compact range request: {:?}", request.get_ref());

        // Placeholder implementation
        let response = CompactRangeResponse {
            success: true,
            error_message: String::new(),
        };

        Ok(Response::new(response))
    }

    async fn get_property(
        &self,
        request: Request<GetPropertyRequest>,
    ) -> Result<Response<GetPropertyResponse>, Status> {
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

    async fn backup_database(
        &self,
        request: Request<BackupDatabaseRequest>,
    ) -> Result<Response<BackupDatabaseResponse>, Status> {
        debug!("Backup database request: {:?}", request.get_ref());

        // Placeholder implementation
        let response = BackupDatabaseResponse {
            success: true,
            error_message: String::new(),
        };

        Ok(Response::new(response))
    }

    async fn restore_database(
        &self,
        request: Request<RestoreDatabaseRequest>,
    ) -> Result<Response<RestoreDatabaseResponse>, Status> {
        debug!("Restore database request: {:?}", request.get_ref());

        // Placeholder implementation
        let response = RestoreDatabaseResponse {
            success: true,
            error_message: String::new(),
        };

        Ok(Response::new(response))
    }

    // Execution operations (delegated to execution module)
    async fn execute_transaction(
        &self,
        request: Request<ExecuteTransactionRequest>,
    ) -> Result<Response<ExecuteTransactionResponse>, Status> {
        debug!("Execute transaction request: {:?}", request.get_ref());

        let req = request.get_ref();

        // Parse pre-execution AEXT snapshots for hybrid mode
        let pre_exec_aext_map = parse_pre_execution_aext(&req.pre_execution_aext);
        if !pre_exec_aext_map.is_empty() {
            debug!(
                "Parsed {} pre-execution AEXT snapshots for hybrid mode",
                pre_exec_aext_map.len()
            );
        }

        // Get the execution module
        let execution_module = self.get_execution_module()?;

        // Downcast to the concrete execution module type
        let execution_module = execution_module
            .as_any()
            .downcast_ref::<ExecutionModule>()
            .ok_or_else(|| Status::internal("Failed to downcast execution module"))?;

        // Convert protobuf types to execution types
        let (transaction, tx_kind) = match self
            .convert_protobuf_transaction(req.transaction.as_ref(), req.transaction_bytes_size)
        {
            Ok((tx, kind)) => {
                debug!("Converted transaction - gas_limit: {}, gas_price: {}, data_len: {}, kind: {:?}", 
                       tx.gas_limit, tx.gas_price, tx.data.len(), kind);
                (tx, kind)
            }
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
                        freeze_changes: vec![],
                        global_resource_changes: vec![],
                        trc10_changes: vec![],
                        vote_changes: vec![],
                        withdraw_changes: vec![],
                        tron_transaction_result: vec![],
                        contract_address: vec![],
                    }),
                    success: false,
                    error_message: format!("Transaction conversion error: {}", e),
                    write_mode: 0,
                    touched_keys: vec![],
                }));
            }
        };

        let context = match self.convert_protobuf_context(req.context.as_ref()) {
            Ok(ctx) => {
                debug!(
                    "Converted context - block_gas_limit: {}, energy_price: {}",
                    ctx.block_gas_limit, ctx.energy_price
                );
                ctx
            }
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
                        freeze_changes: vec![],
                        global_resource_changes: vec![],
                        trc10_changes: vec![],
                        vote_changes: vec![],
                        withdraw_changes: vec![],
                        tron_transaction_result: vec![],
                        contract_address: vec![],
                    }),
                    success: false,
                    error_message: format!("Context conversion error: {}", e),
                    write_mode: 0,
                    touched_keys: vec![],
                }));
            }
        };

        // Get the storage engine and create a unified storage adapter
        let storage_engine = self.get_storage_engine()?;

        // Phase B: Check if rust_persist_enabled to use buffered writes
        let rust_persist_enabled = self
            .get_execution_config()
            .map(|cfg| cfg.remote.rust_persist_enabled)
            .unwrap_or(false);

        // Create storage adapter with optional write buffer.
        //
        // Always buffer NON_VM execution to match java-tron's transactional semantics:
        // if execution returns an error, no partial writes should be persisted.
        let use_buffered_writes =
            rust_persist_enabled || matches!(tx_kind, crate::backend::TxKind::NonVm);
        let (mut storage_adapter, write_buffer) = if use_buffered_writes {
            let (adapter, buffer) =
                tron_backend_execution::EngineBackedEvmStateStore::new_with_buffer(
                    storage_engine.clone(),
                );
            if rust_persist_enabled {
                info!("Phase B: Using buffered writes (rust_persist_enabled=true)");
            } else {
                info!("Using buffered writes for NON_VM atomicity");
            }
            (adapter, Some(buffer))
        } else {
            let adapter =
                tron_backend_execution::EngineBackedEvmStateStore::new(storage_engine.clone());
            (adapter, None)
        };

        // Log blackhole balance before execution
        let blackhole_balance_before = if let Ok(Some(blackhole_addr)) =
            storage_adapter.get_blackhole_address()
        {
            let balance = storage_adapter
                .get_account(&blackhole_addr)
                .ok()
                .flatten()
                .map(|acc| acc.balance)
                .unwrap_or(revm_primitives::U256::ZERO);
            let blackhole_addr_array: [u8; 20] = blackhole_addr.as_slice().try_into().unwrap();
            let blackhole_base58 = to_tron_address(&blackhole_addr_array);
            let from_addr_array: [u8; 20] = transaction.from.as_slice().try_into().unwrap();
            let from_addr_base58 = to_tron_address(&from_addr_array);
            let contract_type_str = transaction
                .metadata
                .contract_type
                .as_ref()
                .map(|ct| format!("{:?}", ct))
                .unwrap_or_else(|| "UNKNOWN".to_string());

            let tx_id = req
                .context
                .as_ref()
                .map(|c| c.transaction_id.as_str())
                .unwrap_or("");
            info!("Blackhole balance BEFORE execution: {} SUN (address: {}) - block: {}, txId: {}, tx from: {}, contract_type: {}",
                  balance, blackhole_base58, context.block_number, tx_id, from_addr_base58, contract_type_str);
            Some((blackhole_addr, balance, blackhole_base58))
        } else {
            warn!("Blackhole address not configured, skipping balance logging");
            None
        };

        // Phase 3: Branch execution based on transaction kind
        let execution_result = match tx_kind {
            crate::backend::TxKind::NonVm => {
                info!("Executing NON_VM transaction with contract type dispatch");
                // Execute non-VM transaction with contract type dispatch
                match self.execute_non_vm_contract(&mut storage_adapter, &transaction, &context) {
                    Ok(result) => {
                        info!("Non-VM contract executed successfully - energy_used: {}, bandwidth_used: {}, state_changes: {}",
                              result.energy_used, result.bandwidth_used, result.state_changes.len());
                        Ok(result)
                    }
                    Err(e) => {
                        error!("Non-VM contract execution failed: {}", e);
                        Err(anyhow::anyhow!("Non-VM execution error: {}", e))
                    }
                }
            }
            crate::backend::TxKind::Vm => {
                info!("Executing VM transaction via EVM");

                // TRON Parity Fix: Check if this is likely a non-VM transaction before execution (fallback heuristic)
                let is_non_vm = self.is_likely_non_vm_transaction(&transaction, &storage_adapter);

                // Phase 2.I L2: Check if this is a CreateSmartContract for post-EVM metadata persistence
                let is_create_smart_contract = transaction
                    .metadata
                    .contract_type
                    .as_ref()
                    .map(|ct| *ct == tron_backend_execution::TronContractType::CreateSmartContract)
                    .unwrap_or(false);

                // Execute the transaction using the database-specific storage adapter
                match execution_module.execute_transaction_with_storage(
                    storage_adapter,
                    &transaction,
                    &context,
                ) {
                    Ok(mut result) => {
                        // TRON Parity Fix: Apply non-VM heuristic to set energy_used = 0 for non-VM transactions
                        if is_non_vm {
                            debug!("Detected likely non-VM transaction (empty data, no code at 'to' address) - setting energy_used = 0");
                            debug!(
                                "Original energy_used: {}, from: {:?}, to: {:?}, value: {}",
                                result.energy_used,
                                transaction.from,
                                transaction.to,
                                transaction.value
                            );
                            result.energy_used = 0;
                        } else {
                            debug!(
                                "Detected VM transaction - keeping original energy_used: {}",
                                result.energy_used
                            );
                        }

                        // TRON Phase 2: Apply fee post-processing based on configuration
                        if let Err(e) = self.apply_fee_post_processing(
                            &mut result,
                            &transaction,
                            &context,
                            is_non_vm,
                        ) {
                            warn!(
                                "Fee post-processing failed: {}, continuing with original result",
                                e
                            );
                            // Continue with original result
                        }

                        // Phase 2.I L2: Persist SmartContract metadata after successful contract creation
                        // Note: We need to create a new storage adapter since execute_transaction_with_storage consumes it
                        if is_create_smart_contract
                            && result.success
                            && result.contract_address.is_some()
                        {
                            // Phase B: Use buffered adapter if rust_persist_enabled to track metadata writes
                            let mut persist_storage_adapter = if let Some(ref buffer) = write_buffer
                            {
                                let mut adapter =
                                    tron_backend_execution::EngineBackedEvmStateStore::new(
                                        storage_engine.clone(),
                                    );
                                adapter.set_write_buffer(buffer.clone());
                                adapter
                            } else {
                                tron_backend_execution::EngineBackedEvmStateStore::new(
                                    storage_engine.clone(),
                                )
                            };
                            if let Err(e) = self.persist_smart_contract_metadata(
                                &mut persist_storage_adapter,
                                &transaction,
                                &context,
                                result.contract_address.as_ref().unwrap(),
                            ) {
                                warn!("Failed to persist SmartContract metadata: {}", e);
                                // Continue - contract was created, but metadata wasn't persisted
                                // This is recoverable as Java can still access via embedded mode
                            }

                            // Phase 2: Emit TRC-10 call_token_value transfer if applicable
                            // Java's VMActuator transfers TRC-10 tokens from owner to contract before execution.
                            // We emit the Trc10Change on success so Java can apply the token transfer.
                            match self.extract_create_contract_trc10_transfer(
                                &persist_storage_adapter,
                                &transaction,
                                result.contract_address.as_ref().unwrap(),
                            ) {
                                Ok(Some(trc10_change)) => {
                                    result.trc10_changes.push(trc10_change);
                                    debug!("Added TRC-10 transfer change for CreateSmartContract");
                                }
                                Ok(None) => {
                                    // No TRC-10 transfer needed (call_token_value <= 0 or TRC-10 disabled)
                                }
                                Err(e) => {
                                    warn!("Failed to extract TRC-10 transfer for CreateSmartContract: {}", e);
                                    // Continue - contract was created, but TRC-10 transfer wasn't emitted
                                }
                            }
                        }

                        Ok(result)
                    }
                    Err(e) => Err(e),
                }
            }
        };

        // Log blackhole balance after execution and compute delta
        if let Some((blackhole_addr, balance_before, blackhole_base58)) =
            blackhole_balance_before.as_ref()
        {
            // Create a fresh storage adapter to query post-execution state
            let post_exec_adapter =
                tron_backend_execution::EngineBackedEvmStateStore::new(storage_engine.clone());

            if let Ok(Some(account)) = post_exec_adapter.get_account(blackhole_addr) {
                let balance_after = account.balance;
                let delta_signed = if balance_after >= *balance_before {
                    let delta = balance_after.saturating_sub(*balance_before);
                    format!("+{}", delta)
                } else {
                    let delta = balance_before.saturating_sub(balance_after);
                    format!("-{}", delta)
                };
                let from_addr_array: [u8; 20] = transaction.from.as_slice().try_into().unwrap();
                let from_addr_base58 = to_tron_address(&from_addr_array);
                let contract_type_str = transaction
                    .metadata
                    .contract_type
                    .as_ref()
                    .map(|ct| format!("{:?}", ct))
                    .unwrap_or_else(|| "UNKNOWN".to_string());

                let tx_id = req
                    .context
                    .as_ref()
                    .map(|c| c.transaction_id.as_str())
                    .unwrap_or("");
                info!("Blackhole balance AFTER execution: {} SUN (address: {}, delta: {} SUN) - block: {}, txId: {}, tx from: {}, contract_type: {}",
                      balance_after, blackhole_base58, delta_signed, context.block_number, tx_id, from_addr_base58, contract_type_str);
            } else {
                warn!(
                    "Blackhole account disappeared after execution (address: {})",
                    blackhole_base58
                );
            }
        }

        // Handle execution result
        match execution_result {
            Ok(result) => {
                info!(
                    "Transaction executed successfully - energy_used: {}, bandwidth_used: {}",
                    result.energy_used, result.bandwidth_used
                );

                // Phase B: Commit buffer and extract touched_keys if rust_persist_enabled
                let (touched_keys, write_mode) = if let Some(ref buffer) = write_buffer {
                    match buffer.lock() {
                        Ok(mut locked_buffer) => {
                            // Only commit if execution was successful
                            if result.success {
                                // Capture touched keys BEFORE commit clears the buffer.
                                let keys = locked_buffer.touched_keys().to_vec();
                                let op_count = locked_buffer.operation_count();
                                match locked_buffer.commit(&storage_engine) {
                                    Ok(()) => {
                                        info!(
                                            "Phase B: Committed {} writes, {} touched keys",
                                            op_count,
                                            keys.len()
                                        );
                                        (Some(keys), 1) // WRITE_MODE_PERSISTED
                                    }
                                    Err(e) => {
                                        error!("Phase B: Failed to commit buffer: {}", e);
                                        // Fallback to compute-only mode on commit failure
                                        (None, 0)
                                    }
                                }
                            } else {
                                // Execution failed/reverted - don't commit, drop buffer
                                info!(
                                    "Phase B: Dropping buffer without commit (execution reverted)"
                                );
                                (None, 0)
                            }
                        }
                        Err(e) => {
                            error!("Phase B: Failed to lock buffer: {}", e);
                            (None, 0)
                        }
                    }
                } else {
                    // No buffer (compute-only mode)
                    (None, 0)
                };

                let response = self.convert_execution_result_to_protobuf(
                    result,
                    &pre_exec_aext_map,
                    touched_keys.as_deref(),
                    write_mode,
                );
                Ok(Response::new(response))
            }
            Err(e) => {
                let error_str = format!("{}", e);
                error!("Transaction execution failed: {}", error_str);

                // Check if it's a gas-related error
                if error_str.contains("CallGasCostMoreThanGasLimit")
                    || error_str.contains("OutOfGas")
                {
                    warn!(
                        "Gas limit issue detected - tx.gas_limit: {}, block.gas_limit: {}",
                        transaction.gas_limit, context.block_gas_limit
                    );
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
                        freeze_changes: vec![],
                        global_resource_changes: vec![],
                        trc10_changes: vec![],
                        vote_changes: vec![],
                        withdraw_changes: vec![],
                        tron_transaction_result: vec![],
                        contract_address: vec![],
                    }),
                    success: false,
                    error_message: format!("Execution error: {}", e),
                    write_mode: 0,
                    touched_keys: vec![],
                }))
            }
        }
    }

    async fn call_contract(
        &self,
        request: Request<CallContractRequest>,
    ) -> Result<Response<CallContractResponse>, Status> {
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
        let storage_adapter =
            tron_backend_execution::EngineBackedEvmStateStore::new(storage_engine.clone());

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

    async fn estimate_energy(
        &self,
        request: Request<EstimateEnergyRequest>,
    ) -> Result<Response<EstimateEnergyResponse>, Status> {
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
        let _transaction = match self.convert_protobuf_transaction(req.transaction.as_ref(), 0) {
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
        let storage_adapter =
            tron_backend_execution::EngineBackedEvmStateStore::new(storage_engine.clone());

        // Estimate energy using the database-specific storage adapter
        // Convert protobuf types to execution types (for estimate_energy, we don't need tx_kind)
        let (transaction_only, _) =
            match self.convert_protobuf_transaction(req.transaction.as_ref(), 0) {
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

        match execution_module.estimate_energy_with_storage(
            storage_adapter,
            &transaction_only,
            &context,
        ) {
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

    async fn get_code(
        &self,
        request: Request<GetCodeRequest>,
    ) -> Result<Response<GetCodeResponse>, Status> {
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

    async fn get_storage_at(
        &self,
        request: Request<GetStorageAtRequest>,
    ) -> Result<Response<GetStorageAtResponse>, Status> {
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

    async fn get_nonce(
        &self,
        request: Request<GetNonceRequest>,
    ) -> Result<Response<GetNonceResponse>, Status> {
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

    async fn get_balance(
        &self,
        request: Request<GetBalanceRequest>,
    ) -> Result<Response<GetBalanceResponse>, Status> {
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

    async fn create_evm_snapshot(
        &self,
        request: Request<CreateEvmSnapshotRequest>,
    ) -> Result<Response<CreateEvmSnapshotResponse>, Status> {
        debug!("Create EVM snapshot request: {:?}", request.get_ref());

        // Placeholder implementation
        let response = CreateEvmSnapshotResponse {
            snapshot_id: uuid::Uuid::new_v4().to_string(),
            success: true,
            error_message: String::new(),
        };

        Ok(Response::new(response))
    }

    async fn revert_to_evm_snapshot(
        &self,
        request: Request<RevertToEvmSnapshotRequest>,
    ) -> Result<Response<RevertToEvmSnapshotResponse>, Status> {
        debug!("Revert to EVM snapshot request: {:?}", request.get_ref());

        // Placeholder implementation
        let response = RevertToEvmSnapshotResponse {
            success: true,
            error_message: String::new(),
        };

        Ok(Response::new(response))
    }
}

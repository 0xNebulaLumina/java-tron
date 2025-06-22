use anyhow::Result;
use std::collections::HashMap;
use std::pin::Pin;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream};
use tonic::{Request, Response, Status};
use tracing::{info, error};

use crate::storage::*;
use crate::config::Config;

pub struct StorageServiceImpl {
    engine: StorageEngine,
    _config: Config,
}

impl StorageServiceImpl {
    pub async fn new(config: Config) -> Result<Self> {
        let engine = StorageEngine::new(&config.data_dir)?;
        
        Ok(StorageServiceImpl {
            engine,
            _config: config,
        })
    }
}

#[tonic::async_trait]
impl storage_service_server::StorageService for StorageServiceImpl {
    async fn get(&self, request: Request<GetRequest>) -> Result<Response<GetResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.get(&req.db_name, &req.key) {
            Ok(Some(value)) => {
                Ok(Response::new(GetResponse {
                    value,
                    found: true,
                }))
            }
            Ok(None) => {
                Ok(Response::new(GetResponse {
                    value: Vec::new(),
                    found: false,
                }))
            }
            Err(e) => {
                error!("Get operation failed: {}", e);
                Err(Status::internal(format!("Get operation failed: {}", e)))
            }
        }
    }

    async fn put(&self, request: Request<PutRequest>) -> Result<Response<PutResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.put(&req.db_name, &req.key, &req.value) {
            Ok(()) => Ok(Response::new(PutResponse {})),
            Err(e) => {
                error!("Put operation failed: {}", e);
                Err(Status::internal(format!("Put operation failed: {}", e)))
            }
        }
    }

    async fn delete(&self, request: Request<DeleteRequest>) -> Result<Response<DeleteResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.delete(&req.db_name, &req.key) {
            Ok(()) => Ok(Response::new(DeleteResponse {})),
            Err(e) => {
                error!("Delete operation failed: {}", e);
                Err(Status::internal(format!("Delete operation failed: {}", e)))
            }
        }
    }

    async fn has(&self, request: Request<HasRequest>) -> Result<Response<HasResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.has(&req.db_name, &req.key) {
            Ok(exists) => Ok(Response::new(HasResponse { exists })),
            Err(e) => {
                error!("Has operation failed: {}", e);
                Err(Status::internal(format!("Has operation failed: {}", e)))
            }
        }
    }

    async fn batch_write(&self, request: Request<BatchWriteRequest>) -> Result<Response<BatchWriteResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.batch_write(&req.db_name, &req.operations) {
            Ok(()) => Ok(Response::new(BatchWriteResponse {})),
            Err(e) => {
                error!("Batch write operation failed: {}", e);
                Err(Status::internal(format!("Batch write operation failed: {}", e)))
            }
        }
    }

    async fn batch_get(&self, request: Request<BatchGetRequest>) -> Result<Response<BatchGetResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.batch_get(&req.db_name, &req.keys) {
            Ok(pairs) => Ok(Response::new(BatchGetResponse { pairs })),
            Err(e) => {
                error!("Batch get operation failed: {}", e);
                Err(Status::internal(format!("Batch get operation failed: {}", e)))
            }
        }
    }

    type IteratorStream = Pin<Box<dyn Stream<Item = Result<IteratorResponse, Status>> + Send>>;

    async fn iterator(&self, request: Request<IteratorRequest>) -> Result<Response<Self::IteratorStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = mpsc::channel(100);
        
        let engine = self.engine.clone();
        let db_name = req.db_name.clone();
        let start_key = req.start_key.clone();
        
        tokio::spawn(async move {
            // This is a simplified implementation. In a real system, you'd want to
            // implement proper streaming with RocksDB iterators
            match engine.get_next(&db_name, &start_key, 1000) {
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
        Ok(Response::new(Box::pin(stream)))
    }

    async fn get_keys_next(&self, request: Request<GetKeysNextRequest>) -> Result<Response<GetKeysNextResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.get_keys_next(&req.db_name, &req.start_key, req.limit) {
            Ok(keys) => Ok(Response::new(GetKeysNextResponse { keys })),
            Err(e) => {
                error!("Get keys next operation failed: {}", e);
                Err(Status::internal(format!("Get keys next operation failed: {}", e)))
            }
        }
    }

    async fn get_values_next(&self, request: Request<GetValuesNextRequest>) -> Result<Response<GetValuesNextResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.get_values_next(&req.db_name, &req.start_key, req.limit) {
            Ok(values) => Ok(Response::new(GetValuesNextResponse { values })),
            Err(e) => {
                error!("Get values next operation failed: {}", e);
                Err(Status::internal(format!("Get values next operation failed: {}", e)))
            }
        }
    }

    async fn get_next(&self, request: Request<GetNextRequest>) -> Result<Response<GetNextResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.get_next(&req.db_name, &req.start_key, req.limit) {
            Ok(pairs) => Ok(Response::new(GetNextResponse { pairs })),
            Err(e) => {
                error!("Get next operation failed: {}", e);
                Err(Status::internal(format!("Get next operation failed: {}", e)))
            }
        }
    }

    async fn prefix_query(&self, request: Request<PrefixQueryRequest>) -> Result<Response<PrefixQueryResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.prefix_query(&req.db_name, &req.prefix) {
            Ok(pairs) => Ok(Response::new(PrefixQueryResponse { pairs })),
            Err(e) => {
                error!("Prefix query operation failed: {}", e);
                Err(Status::internal(format!("Prefix query operation failed: {}", e)))
            }
        }
    }

    async fn init_db(&self, request: Request<InitDbRequest>) -> Result<Response<InitDbResponse>, Status> {
        let req = request.into_inner();
        
        match req.config {
            Some(config) => {
                match self.engine.init_db(&req.db_name, &config) {
                    Ok(()) => {
                        info!("Initialized database: {}", req.db_name);
                        Ok(Response::new(InitDbResponse {}))
                    }
                    Err(e) => {
                        error!("Init DB operation failed: {}", e);
                        Err(Status::internal(format!("Init DB operation failed: {}", e)))
                    }
                }
            }
            None => {
                Err(Status::invalid_argument("Storage config is required"))
            }
        }
    }

    async fn close_db(&self, request: Request<CloseDbRequest>) -> Result<Response<CloseDbResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.close_db(&req.db_name) {
            Ok(()) => Ok(Response::new(CloseDbResponse {})),
            Err(e) => {
                error!("Close DB operation failed: {}", e);
                Err(Status::internal(format!("Close DB operation failed: {}", e)))
            }
        }
    }

    async fn reset_db(&self, request: Request<ResetDbRequest>) -> Result<Response<ResetDbResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.reset_db(&req.db_name) {
            Ok(()) => Ok(Response::new(ResetDbResponse {})),
            Err(e) => {
                error!("Reset DB operation failed: {}", e);
                Err(Status::internal(format!("Reset DB operation failed: {}", e)))
            }
        }
    }

    async fn is_alive(&self, request: Request<IsAliveRequest>) -> Result<Response<IsAliveResponse>, Status> {
        let req = request.into_inner();
        let alive = self.engine.is_alive(&req.db_name);
        Ok(Response::new(IsAliveResponse { alive }))
    }

    async fn size(&self, request: Request<SizeRequest>) -> Result<Response<SizeResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.size(&req.db_name) {
            Ok(size) => Ok(Response::new(SizeResponse { size })),
            Err(e) => {
                error!("Size operation failed: {}", e);
                Err(Status::internal(format!("Size operation failed: {}", e)))
            }
        }
    }

    async fn is_empty(&self, request: Request<IsEmptyRequest>) -> Result<Response<IsEmptyResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.is_empty(&req.db_name) {
            Ok(empty) => Ok(Response::new(IsEmptyResponse { empty })),
            Err(e) => {
                error!("Is empty operation failed: {}", e);
                Err(Status::internal(format!("Is empty operation failed: {}", e)))
            }
        }
    }

    async fn begin_transaction(&self, request: Request<BeginTransactionRequest>) -> Result<Response<BeginTransactionResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.begin_transaction(&req.db_name) {
            Ok(transaction_id) => Ok(Response::new(BeginTransactionResponse { transaction_id })),
            Err(e) => {
                error!("Begin transaction operation failed: {}", e);
                Err(Status::internal(format!("Begin transaction operation failed: {}", e)))
            }
        }
    }

    async fn commit_transaction(&self, request: Request<CommitTransactionRequest>) -> Result<Response<CommitTransactionResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.commit_transaction(&req.transaction_id) {
            Ok(()) => Ok(Response::new(CommitTransactionResponse {})),
            Err(e) => {
                error!("Commit transaction operation failed: {}", e);
                Err(Status::internal(format!("Commit transaction operation failed: {}", e)))
            }
        }
    }

    async fn rollback_transaction(&self, request: Request<RollbackTransactionRequest>) -> Result<Response<RollbackTransactionResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.rollback_transaction(&req.transaction_id) {
            Ok(()) => Ok(Response::new(RollbackTransactionResponse {})),
            Err(e) => {
                error!("Rollback transaction operation failed: {}", e);
                Err(Status::internal(format!("Rollback transaction operation failed: {}", e)))
            }
        }
    }

    async fn create_snapshot(&self, request: Request<CreateSnapshotRequest>) -> Result<Response<CreateSnapshotResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.create_snapshot(&req.db_name) {
            Ok(snapshot_id) => Ok(Response::new(CreateSnapshotResponse { snapshot_id })),
            Err(e) => {
                error!("Create snapshot operation failed: {}", e);
                Err(Status::internal(format!("Create snapshot operation failed: {}", e)))
            }
        }
    }

    async fn delete_snapshot(&self, request: Request<DeleteSnapshotRequest>) -> Result<Response<DeleteSnapshotResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.delete_snapshot(&req.snapshot_id) {
            Ok(()) => Ok(Response::new(DeleteSnapshotResponse {})),
            Err(e) => {
                error!("Delete snapshot operation failed: {}", e);
                Err(Status::internal(format!("Delete snapshot operation failed: {}", e)))
            }
        }
    }

    async fn get_from_snapshot(&self, request: Request<GetFromSnapshotRequest>) -> Result<Response<GetFromSnapshotResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.get_from_snapshot(&req.snapshot_id, &req.key) {
            Ok(Some(value)) => Ok(Response::new(GetFromSnapshotResponse { value, found: true })),
            Ok(None) => Ok(Response::new(GetFromSnapshotResponse { value: Vec::new(), found: false })),
            Err(e) => {
                error!("Get from snapshot operation failed: {}", e);
                Err(Status::internal(format!("Get from snapshot operation failed: {}", e)))
            }
        }
    }

    async fn get_stats(&self, request: Request<GetStatsRequest>) -> Result<Response<GetStatsResponse>, Status> {
        let req = request.into_inner();
        
        match self.engine.get_stats(&req.db_name) {
            Ok(stats) => Ok(Response::new(GetStatsResponse { stats: Some(stats) })),
            Err(e) => {
                error!("Get stats operation failed: {}", e);
                Err(Status::internal(format!("Get stats operation failed: {}", e)))
            }
        }
    }

    async fn list_databases(&self, _request: Request<ListDatabasesRequest>) -> Result<Response<ListDatabasesResponse>, Status> {
        let db_names = self.engine.list_databases();
        Ok(Response::new(ListDatabasesResponse { db_names }))
    }

    async fn health_check(&self, _request: Request<HealthCheckRequest>) -> Result<Response<HealthCheckResponse>, Status> {
        let status = self.engine.health_check();
        Ok(Response::new(HealthCheckResponse { status: status as i32 }))
    }

    type StreamMetricsStream = Pin<Box<dyn Stream<Item = Result<MetricsResponse, Status>> + Send>>;

    async fn stream_metrics(&self, request: Request<StreamMetricsRequest>) -> Result<Response<Self::StreamMetricsStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = mpsc::channel(10);
        
        let engine = self.engine.clone();
        let db_name = req.db_name.clone();
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            
            loop {
                interval.tick().await;
                
                if db_name.is_empty() {
                    // Stream metrics for all databases
                    let db_names = engine.list_databases();
                    for name in db_names {
                        if let Ok(stats) = engine.get_stats(&name) {
                            let mut metrics = HashMap::new();
                            metrics.insert("total_keys".to_string(), stats.total_keys as f64);
                            metrics.insert("total_size".to_string(), stats.total_size as f64);
                            
                            let response = MetricsResponse {
                                db_name: name,
                                metrics,
                                timestamp: chrono::Utc::now().timestamp(),
                            };
                            
                            if tx.send(Ok(response)).await.is_err() {
                                return;
                            }
                        }
                    }
                } else {
                    // Stream metrics for specific database
                    if let Ok(stats) = engine.get_stats(&db_name) {
                        let mut metrics = HashMap::new();
                        metrics.insert("total_keys".to_string(), stats.total_keys as f64);
                        metrics.insert("total_size".to_string(), stats.total_size as f64);
                        
                        let response = MetricsResponse {
                            db_name: db_name.clone(),
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
        Ok(Response::new(Box::pin(stream)))
    }
} 
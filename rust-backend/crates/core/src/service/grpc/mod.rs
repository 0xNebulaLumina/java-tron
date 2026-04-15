// gRPC implementation for BackendService
// This module contains the tonic gRPC trait implementation

pub mod address;
pub mod aext;
pub mod conversion;

use self::aext::parse_pre_execution_aext;
use self::conversion::*;
use super::BackendService;
use crate::backend::*;
use prost::Message;
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};
use tron_backend_common::{to_tron_address, HealthStatus};
use tron_backend_execution::protocol::Account as ProtoAccount;
use tron_backend_execution::{
    EvmStateStore, ExecutionModule, ExecutionWriteBuffer, TouchedKey, TronExecutionContext,
    TronTransaction,
};

fn debug_account_filters() -> Vec<Vec<u8>> {
    env::var("TRON_BACKEND_DEBUG_ACCOUNT_HEX")
        .ok()
        .into_iter()
        .flat_map(|raw| {
            raw.split(',')
                .map(str::trim)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let normalized = entry.trim_start_matches("0x");
            match hex::decode(normalized) {
                Ok(bytes) => Some(bytes),
                Err(err) => {
                    warn!(
                        "Ignoring invalid TRON_BACKEND_DEBUG_ACCOUNT_HEX entry '{}': {}",
                        entry, err
                    );
                    None
                }
            }
        })
        .collect()
}

fn decode_account_write(value: &[u8]) -> Option<ProtoAccount> {
    match ProtoAccount::decode(value) {
        Ok(account) => Some(account),
        Err(err) => {
            warn!(
                "Failed to decode account payload during batch-write debug: {}",
                err
            );
            None
        }
    }
}

fn log_filtered_account_batch_write(stage: &str, key: &[u8], value: &[u8]) {
    if let Some(account) = decode_account_write(value) {
        info!(
            "Filtered account batch write {}: key_hex={}, address_hex={}, balance={}, allowance={}, latest_withdraw_time={}, latest_opration_time={}, value_len={}",
            stage,
            hex::encode(key),
            hex::encode(&account.address),
            account.balance,
            account.allowance,
            account.latest_withdraw_time,
            account.latest_opration_time,
            value.len()
        );
    } else {
        info!(
            "Filtered account batch write {}: key_hex={}, undecodable payload, value_len={}",
            stage,
            hex::encode(key),
            value.len()
        );
    }
}

/// Normalize a TRON-format address from a gRPC request into a 20-byte REVM
/// `Address`. Thin wrapper around `address::strip_tron_address_prefix`
/// (the existing shared helper) that converts the returned slice into a
/// concrete `Address`, so every read-path handler agrees on the same
/// "20 raw bytes OR 21 bytes with 0x41/0xa0 prefix" acceptance rule.
///
/// We wrap the shared helper rather than duplicating its logic so that
/// any future change to prefix acceptance (e.g. disabling the legacy
/// 0xa0 testnet prefix) only needs to happen in one place.
fn normalize_tron_address(bytes: &[u8]) -> Result<revm_primitives::Address, String> {
    let stripped = self::address::strip_tron_address_prefix(bytes)?;
    Ok(revm_primitives::Address::from_slice(stripped))
}

/// Canonical error string for Phase 1 "snapshot is unsupported" responses
/// from the execution read-path handlers. Matches the wording used by the
/// storage engine and the Java SPI surfaces so log-grep tooling can find
/// all surfaces with a single search.
fn snapshot_unsupported_error(method: &str) -> String {
    format!(
        "{}: snapshot_id is not supported in close_loop Phase 1 \
         (see planning/close_loop.snapshot.md). Snapshots were previously \
         handled as a fake live-DB read; this has been replaced with an \
         explicit unsupported error.",
        method
    )
}

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

        // close_loop Section 3.1: branch on transaction_id. An empty string
        // means "direct write against the base DB" (the historical behavior).
        // A non-empty transaction_id routes through the per-tx write buffer
        // in the engine; unknown transaction ids are rejected with an
        // explicit error per planning/close_loop.storage_transactions.md.
        let result = if req.transaction_id.is_empty() {
            debug!(
                "Direct put: db={}, key_len={}",
                req.database,
                req.key.len()
            );
            engine.put(&req.database, &req.key, &req.value)
        } else {
            debug!(
                "Transactional put: db={}, tx={}, key_len={}",
                req.database,
                req.transaction_id,
                req.key.len()
            );
            engine.put_in_tx(&req.transaction_id, &req.database, &req.key, &req.value)
        };

        match result {
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

        // close_loop Section 3.1: same direct vs transactional branching as `put`.
        let result = if req.transaction_id.is_empty() {
            debug!(
                "Direct delete: db={}, key_len={}",
                req.database,
                req.key.len()
            );
            engine.delete(&req.database, &req.key)
        } else {
            debug!(
                "Transactional delete: db={}, tx={}, key_len={}",
                req.database,
                req.transaction_id,
                req.key.len()
            );
            engine.delete_in_tx(&req.transaction_id, &req.database, &req.key)
        };

        match result {
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

        let debug_filters = if req.database == "account" {
            debug_account_filters()
        } else {
            Vec::new()
        };
        let filtered_debug_ops: Vec<(i32, Vec<u8>, Vec<u8>)> = if debug_filters.is_empty() {
            Vec::new()
        } else {
            operations
                .iter()
                .filter(|op| {
                    debug_filters
                        .iter()
                        .any(|filter| filter.as_slice() == op.key.as_slice())
                })
                .map(|op| (op.r#type, op.key.clone(), op.value.clone()))
                .collect()
        };

        for (op_type, key, value) in &filtered_debug_ops {
            match *op_type {
                0 => log_filtered_account_batch_write("request", key, value),
                1 => info!(
                    "Filtered account batch delete request: key_hex={}",
                    hex::encode(key)
                ),
                other => warn!(
                    "Filtered account batch write request has unknown op type {} for key {}",
                    other,
                    hex::encode(key)
                ),
            }
        }

        // close_loop Section 3.1: branch on transaction_id (same contract as `put` / `delete`).
        // The filtered-debug readback paths below are only meaningful for direct
        // writes — for transactional writes, the engine has only buffered the
        // operations and the readback would not see anything until commit. We
        // skip the readback diagnostics in the transactional branch but still
        // log a debug line so operator-facing tracing is consistent.
        let result = if req.transaction_id.is_empty() {
            debug!(
                "Direct batch_write: db={}, op_count={}",
                req.database,
                operations.len()
            );
            engine.batch_write(&req.database, &operations)
        } else {
            debug!(
                "Transactional batch_write: db={}, tx={}, op_count={}",
                req.database,
                req.transaction_id,
                operations.len()
            );
            engine.batch_write_in_tx(&req.transaction_id, &req.database, &operations)
        };

        match result {
            Ok(()) => {
                // Read-back diagnostics only make sense for direct writes —
                // a transactional write only buffers the operation, so a live
                // `engine.get` would (correctly) report it missing until the
                // commit lands. Skip the diagnostics in the transactional
                // branch to avoid spurious "missing on readback" warnings.
                if req.transaction_id.is_empty() {
                    for (op_type, key, value) in &filtered_debug_ops {
                        match *op_type {
                            0 => match engine.get(&req.database, key) {
                                Ok(Some(stored)) => {
                                    log_filtered_account_batch_write("readback", key, &stored);
                                    if stored != *value {
                                        error!(
                                            "Filtered account batch write readback mismatch: key_hex={}, expected_len={}, actual_len={}",
                                            hex::encode(key),
                                            value.len(),
                                            stored.len()
                                        );
                                    }
                                }
                                Ok(None) => {
                                    error!(
                                        "Filtered account batch write missing on readback: key_hex={}",
                                        hex::encode(key)
                                    );
                                }
                                Err(err) => {
                                    error!(
                                        "Filtered account batch write readback failed: key_hex={}, error={}",
                                        hex::encode(key),
                                        err
                                    );
                                }
                            },
                            1 => match engine.get(&req.database, key) {
                                Ok(Some(_)) => {
                                    error!(
                                        "Filtered account batch delete still present after readback: key_hex={}",
                                        hex::encode(key)
                                    );
                                }
                                Ok(None) => {
                                    info!(
                                        "Filtered account batch delete confirmed: key_hex={}",
                                        hex::encode(key)
                                    );
                                }
                                Err(err) => {
                                    error!(
                                        "Filtered account batch delete readback failed: key_hex={}, error={}",
                                        hex::encode(key),
                                        err
                                    );
                                }
                            },
                            _ => {}
                        }
                    }
                }

                // `operations_applied` semantics (close_loop Section 3.1):
                // - Direct branch (empty transaction_id): the ops are persisted
                //   as part of the RocksDB WriteBatch, so "applied" matches
                //   `operations.len()`.
                // - Transactional branch (non-empty transaction_id): the ops
                //   are only buffered; nothing has been written to RocksDB
                //   yet. Return `0` so a caller cannot mistake a successful
                //   buffer append for a persisted commit. Persistence of the
                //   buffered batch happens later at `commitTransaction` time;
                //   the commit response does not itself expose an apply count,
                //   so callers that need to audit how many operations were
                //   committed must track it on the Java side.
                let operations_applied = if req.transaction_id.is_empty() {
                    operations.len() as i32
                } else {
                    0
                };
                let response = BatchWriteResponse {
                    success: true,
                    error_message: String::new(),
                    operations_applied,
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
        // Capture the DB-detected address prefix before storage_adapter is moved into execution.
        let address_prefix = storage_adapter.address_prefix();

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

                // Phase B: Commit buffer and extract touched_keys if rust_persist_enabled.
                //
                // Parity rule (close_loop): once the handler decided to buffer
                // writes (rust_persist_enabled OR NonVm atomicity), a commit or
                // lock failure is a hard error. Silently downgrading
                // `write_mode` to 0 would flip ownership of the final state back
                // to Java without Java being told, so the Java side would
                // re-apply sidecars that may or may not match what Rust already
                // partially wrote. We fail the RPC instead and let the Java
                // caller abort / retry.
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
                                        let msg = format!(
                                            "Buffer commit failed after successful execution: {}",
                                            e
                                        );
                                        return Ok(Response::new(ExecuteTransactionResponse {
                                            result: Some(ExecutionResult {
                                                status:
                                                    execution_result::Status::TronSpecificError
                                                        as i32,
                                                return_data: vec![],
                                                energy_used: 0,
                                                energy_refunded: 0,
                                                state_changes: vec![],
                                                logs: vec![],
                                                error_message: msg.clone(),
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
                                            error_message: msg,
                                            write_mode: 0,
                                            touched_keys: vec![],
                                        }));
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
                            let msg = format!("Buffer lock failed after successful execution: {}", e);
                            return Ok(Response::new(ExecuteTransactionResponse {
                                result: Some(ExecutionResult {
                                    status: execution_result::Status::TronSpecificError as i32,
                                    return_data: vec![],
                                    energy_used: 0,
                                    energy_refunded: 0,
                                    state_changes: vec![],
                                    logs: vec![],
                                    error_message: msg.clone(),
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
                                error_message: msg,
                                write_mode: 0,
                                touched_keys: vec![],
                            }));
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
                    address_prefix,
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
                    // close_loop iter 6: request-conversion failures
                    // are handler-level — the VM never runs.
                    status: call_contract_response::Status::HandlerError as i32,
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
                    // close_loop iter 6: same as above — the VM never ran.
                    status: call_contract_response::Status::HandlerError as i32,
                }));
            }
        };

        // Get the storage engine and create a unified storage adapter
        let storage_engine = self.get_storage_engine()?;
        let storage_adapter =
            tron_backend_execution::EngineBackedEvmStateStore::new(storage_engine.clone());

        // Call the contract using the database-specific storage adapter.
        //
        // close_loop Phase 1 — Section 2.1:
        //   - The request converter above now prefers the full
        //     `CallContractRequest.transaction` field (iter 6) so
        //     value/gas_limit/metadata all round-trip from Java.
        //   - The response now carries a structured `status` enum
        //     (iter 6) instead of forcing the Java side to match on
        //     error-message string prefixes. `success`/`error_message`
        //     are still populated for backward compatibility with
        //     clients built before iter 6, but new clients should read
        //     `status` directly.
        match execution_module.call_contract_with_storage(storage_adapter, &transaction, &context) {
            Ok(result) => {
                // Classify the execution outcome into the new Status
                // enum. The Rust side has access to
                // `TronExecutionResult.success` and the error string
                // that REVM/tron_evm.rs set; the mapping mirrors what
                // the Java bridge used to infer from string prefixes.
                let status = if result.success {
                    call_contract_response::Status::Success
                } else {
                    match result.error.as_deref() {
                        Some("Call reverted") => call_contract_response::Status::Revert,
                        Some(msg) if msg.starts_with("Call halted:") => {
                            call_contract_response::Status::Halt
                        }
                        // An `Ok(result)` with `success=false` and an
                        // error string that is neither "Call reverted"
                        // nor "Call halted:*" should not happen today,
                        // but if it does, report it as a handler-level
                        // error so the Java side can surface it without
                        // guessing.
                        _ => call_contract_response::Status::HandlerError,
                    }
                };
                let response = CallContractResponse {
                    return_data: result.return_data.to_vec(),
                    success: result.success,
                    error_message: result.error.clone().unwrap_or_default(),
                    energy_used: result.energy_used as i64,
                    status: status as i32,
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
                    // `Err(_)` from call_contract_with_storage is a
                    // handler-level failure — the VM never ran.
                    status: call_contract_response::Status::HandlerError as i32,
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

    /// close_loop Phase 1 — Section 2.2 execution read-path closure.
    ///
    /// Returns the contract byte code stored at `address`. Reads flow through
    /// the live storage engine; per `close_loop.snapshot.md`, any non-empty
    /// `snapshot_id` is rejected with an explicit unsupported error rather
    /// than silently reading from the live DB.
    async fn get_code(
        &self,
        request: Request<GetCodeRequest>,
    ) -> Result<Response<GetCodeResponse>, Status> {
        debug!("Get code request: {:?}", request.get_ref());
        let req = request.into_inner();

        if !req.snapshot_id.is_empty() {
            return Ok(Response::new(GetCodeResponse {
                code: vec![],
                found: false,
                success: false,
                error_message: snapshot_unsupported_error("get_code"),
            }));
        }

        let address = match normalize_tron_address(&req.address) {
            Ok(addr) => addr,
            Err(e) => {
                return Ok(Response::new(GetCodeResponse {
                    code: vec![],
                    found: false,
                    success: false,
                    error_message: format!("Invalid address: {}", e),
                }));
            }
        };

        let storage_engine = self.get_storage_engine()?;
        let adapter =
            tron_backend_execution::EngineBackedEvmStateStore::new(storage_engine.clone());

        match adapter.get_code(&address) {
            Ok(Some(bytecode)) => Ok(Response::new(GetCodeResponse {
                code: bytecode.bytes().to_vec(),
                found: true,
                success: true,
                error_message: String::new(),
            })),
            Ok(None) => Ok(Response::new(GetCodeResponse {
                code: vec![],
                found: false,
                success: true,
                error_message: String::new(),
            })),
            Err(e) => {
                error!("get_code engine read failed: {}", e);
                Ok(Response::new(GetCodeResponse {
                    code: vec![],
                    found: false,
                    success: false,
                    error_message: format!("get_code engine error: {}", e),
                }))
            }
        }
    }

    /// close_loop Phase 1 — Section 2.2.
    ///
    /// Returns the VM storage slot at `(address, key)`. The `key` is
    /// interpreted as a 32-byte big-endian storage index; shorter inputs
    /// are left-padded with zeros, longer inputs are rejected. Non-empty
    /// `snapshot_id` is rejected per `close_loop.snapshot.md`.
    async fn get_storage_at(
        &self,
        request: Request<GetStorageAtRequest>,
    ) -> Result<Response<GetStorageAtResponse>, Status> {
        debug!("Get storage at request: {:?}", request.get_ref());
        let req = request.into_inner();

        if !req.snapshot_id.is_empty() {
            return Ok(Response::new(GetStorageAtResponse {
                value: vec![],
                found: false,
                success: false,
                error_message: snapshot_unsupported_error("get_storage_at"),
            }));
        }

        let address = match normalize_tron_address(&req.address) {
            Ok(addr) => addr,
            Err(e) => {
                return Ok(Response::new(GetStorageAtResponse {
                    value: vec![],
                    found: false,
                    success: false,
                    error_message: format!("Invalid address: {}", e),
                }));
            }
        };

        if req.key.len() > 32 {
            return Ok(Response::new(GetStorageAtResponse {
                value: vec![],
                found: false,
                success: false,
                error_message: format!(
                    "storage key must be at most 32 bytes, got {}",
                    req.key.len()
                ),
            }));
        }
        let mut key_bytes = [0u8; 32];
        key_bytes[32 - req.key.len()..].copy_from_slice(&req.key);
        let slot = revm_primitives::U256::from_be_bytes(key_bytes);

        let storage_engine = self.get_storage_engine()?;
        let adapter =
            tron_backend_execution::EngineBackedEvmStateStore::new(storage_engine.clone());

        match adapter.get_storage(&address, &slot) {
            Ok(value) => {
                let bytes = value.to_be_bytes::<32>().to_vec();
                let found = value != revm_primitives::U256::ZERO;
                Ok(Response::new(GetStorageAtResponse {
                    value: bytes,
                    found,
                    success: true,
                    error_message: String::new(),
                }))
            }
            Err(e) => {
                error!("get_storage_at engine read failed: {}", e);
                Ok(Response::new(GetStorageAtResponse {
                    value: vec![],
                    found: false,
                    success: false,
                    error_message: format!("get_storage_at engine error: {}", e),
                }))
            }
        }
    }

    /// close_loop Phase 1 — Section 2.2.
    ///
    /// Returns the account nonce (type_code field for TRON accounts) at
    /// `address`. Non-empty `snapshot_id` rejected per snapshot policy.
    async fn get_nonce(
        &self,
        request: Request<GetNonceRequest>,
    ) -> Result<Response<GetNonceResponse>, Status> {
        debug!("Get nonce request: {:?}", request.get_ref());
        let req = request.into_inner();

        if !req.snapshot_id.is_empty() {
            return Ok(Response::new(GetNonceResponse {
                nonce: 0,
                found: false,
                success: false,
                error_message: snapshot_unsupported_error("get_nonce"),
            }));
        }

        let address = match normalize_tron_address(&req.address) {
            Ok(addr) => addr,
            Err(e) => {
                return Ok(Response::new(GetNonceResponse {
                    nonce: 0,
                    found: false,
                    success: false,
                    error_message: format!("Invalid address: {}", e),
                }));
            }
        };

        let storage_engine = self.get_storage_engine()?;
        let adapter =
            tron_backend_execution::EngineBackedEvmStateStore::new(storage_engine.clone());

        match adapter.get_account(&address) {
            Ok(Some(account)) => Ok(Response::new(GetNonceResponse {
                nonce: account.nonce as i64,
                found: true,
                success: true,
                error_message: String::new(),
            })),
            Ok(None) => Ok(Response::new(GetNonceResponse {
                nonce: 0,
                found: false,
                success: true,
                error_message: String::new(),
            })),
            Err(e) => {
                error!("get_nonce engine read failed: {}", e);
                Ok(Response::new(GetNonceResponse {
                    nonce: 0,
                    found: false,
                    success: false,
                    error_message: format!("get_nonce engine error: {}", e),
                }))
            }
        }
    }

    /// close_loop Phase 1 — Section 2.2.
    ///
    /// Returns the account balance (in SUN) at `address`. The balance is
    /// serialized as a 32-byte big-endian blob to preserve the full U256
    /// representation REVM uses internally. Non-empty `snapshot_id` is
    /// rejected per snapshot policy.
    async fn get_balance(
        &self,
        request: Request<GetBalanceRequest>,
    ) -> Result<Response<GetBalanceResponse>, Status> {
        debug!("Get balance request: {:?}", request.get_ref());
        let req = request.into_inner();

        if !req.snapshot_id.is_empty() {
            return Ok(Response::new(GetBalanceResponse {
                balance: vec![],
                found: false,
                success: false,
                error_message: snapshot_unsupported_error("get_balance"),
            }));
        }

        let address = match normalize_tron_address(&req.address) {
            Ok(addr) => addr,
            Err(e) => {
                return Ok(Response::new(GetBalanceResponse {
                    balance: vec![],
                    found: false,
                    success: false,
                    error_message: format!("Invalid address: {}", e),
                }));
            }
        };

        let storage_engine = self.get_storage_engine()?;
        let adapter =
            tron_backend_execution::EngineBackedEvmStateStore::new(storage_engine.clone());

        match adapter.get_account(&address) {
            Ok(Some(account)) => Ok(Response::new(GetBalanceResponse {
                balance: account.balance.to_be_bytes::<32>().to_vec(),
                found: true,
                success: true,
                error_message: String::new(),
            })),
            Ok(None) => Ok(Response::new(GetBalanceResponse {
                balance: vec![0u8; 32],
                found: false,
                success: true,
                error_message: String::new(),
            })),
            Err(e) => {
                error!("get_balance engine read failed: {}", e);
                Ok(Response::new(GetBalanceResponse {
                    balance: vec![],
                    found: false,
                    success: false,
                    error_message: format!("get_balance engine error: {}", e),
                }))
            }
        }
    }

    /// Phase 1: EVM snapshot creation is explicitly UNSUPPORTED.
    ///
    /// See `planning/close_loop.snapshot.md`. The previous placeholder
    /// generated a fresh UUID and returned `success = true`, which let
    /// callers believe they had an isolated point-in-time handle. That
    /// has been replaced with `success = false` plus an explicit error
    /// message so no code path can silently rely on isolation that does
    /// not exist. REVM's intra-transaction journaling still covers
    /// in-VM revert for actual execution — cross-transaction SPI
    /// snapshots are not a Phase 1 consumer.
    async fn create_evm_snapshot(
        &self,
        request: Request<CreateEvmSnapshotRequest>,
    ) -> Result<Response<CreateEvmSnapshotResponse>, Status> {
        debug!(
            "Create EVM snapshot request (rejected as unsupported in Phase 1): {:?}",
            request.get_ref()
        );
        let response = CreateEvmSnapshotResponse {
            snapshot_id: String::new(),
            success: false,
            error_message:
                "EVM snapshot is not supported in close_loop Phase 1 \
                 (see planning/close_loop.snapshot.md). The previous \
                 placeholder returned a synthetic snapshot id without \
                 taking a real point-in-time handle; it has been \
                 replaced with an explicit unsupported error."
                    .to_string(),
        };
        Ok(Response::new(response))
    }

    /// Phase 1: EVM snapshot revert is explicitly UNSUPPORTED.
    ///
    /// See `planning/close_loop.snapshot.md`. The previous placeholder
    /// silently returned `success = true`, which was the "fake success"
    /// state Section 1.4 explicitly bans.
    async fn revert_to_evm_snapshot(
        &self,
        request: Request<RevertToEvmSnapshotRequest>,
    ) -> Result<Response<RevertToEvmSnapshotResponse>, Status> {
        debug!(
            "Revert to EVM snapshot request (rejected as unsupported in Phase 1): {:?}",
            request.get_ref()
        );
        let response = RevertToEvmSnapshotResponse {
            success: false,
            error_message:
                "EVM revert-to-snapshot is not supported in close_loop Phase 1 \
                 (see planning/close_loop.snapshot.md). The previous \
                 placeholder silently returned success; it has been \
                 replaced with an explicit unsupported error."
                    .to_string(),
        };
        Ok(Response::new(response))
    }
}

// =============================================================================
// Tests for iter 4 execution read-path (helpers + handlers)
// =============================================================================
//
// The first half of this module locks the small free-function helpers
// `normalize_tron_address` / `snapshot_unsupported_error` / the storage
// key padding / the U256 balance BE round-trip so that future refactors
// cannot quietly change address acceptance, storage key width, or the
// snapshot-unsupported error string.
//
// The second half (the `#[tokio::test]` block) builds a real
// `BackendService` backed by a real `StorageModule` rooted in a
// `tempfile::TempDir`, then invokes `get_code` / `get_storage_at` /
// `get_nonce` / `get_balance` directly via the tonic `Backend` trait.
// That covers the invariants we care about without a seeded account:
// snapshot rejection, malformed-address rejection, oversized-key
// rejection, and the not-found response shaping (`success = true`
// with `found = false` and zeroed payload).
//
// A seeded-data "happy path" handler test that exercises a real
// Account protobuf write + read is still an open follow-up under
// Section 2.4. It requires constructing AccountCapsule serialization
// from the Rust side, which lives in the execution crate's storage
// adapter and deserves its own test harness.
#[cfg(test)]
mod iter4_read_path_tests {
    use super::*;

    // ---- Address normalization -----------------------------------------

    #[test]
    fn normalize_accepts_20_byte_address() {
        let raw = [0x11u8; 20];
        let addr = normalize_tron_address(&raw).expect("20-byte address accepted");
        assert_eq!(addr.as_slice(), &raw[..]);
    }

    #[test]
    fn normalize_accepts_21_byte_mainnet_prefix() {
        let mut raw = vec![0x41];
        raw.extend_from_slice(&[0x22u8; 20]);
        let addr = normalize_tron_address(&raw).expect("0x41-prefixed address accepted");
        assert_eq!(addr.as_slice(), &raw[1..]);
    }

    #[test]
    fn normalize_accepts_21_byte_testnet_prefix() {
        let mut raw = vec![0xa0];
        raw.extend_from_slice(&[0x33u8; 20]);
        let addr = normalize_tron_address(&raw).expect("0xa0-prefixed address accepted");
        assert_eq!(addr.as_slice(), &raw[1..]);
    }

    #[test]
    fn normalize_rejects_empty_address() {
        let err = normalize_tron_address(&[]).unwrap_err();
        assert!(
            err.contains("expected 20 or 21 bytes"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn normalize_rejects_21_byte_with_bad_prefix() {
        let mut raw = vec![0xff];
        raw.extend_from_slice(&[0x44u8; 20]);
        let err = normalize_tron_address(&raw).unwrap_err();
        assert!(err.contains("expected 20 or 21 bytes"));
    }

    #[test]
    fn normalize_rejects_odd_length() {
        let err = normalize_tron_address(&[0x00u8; 15]).unwrap_err();
        assert!(err.contains("15"));
    }

    #[test]
    fn normalize_rejects_too_long() {
        let err = normalize_tron_address(&[0x00u8; 33]).unwrap_err();
        assert!(err.contains("33"));
    }

    // ---- Snapshot unsupported error string -----------------------------

    #[test]
    fn snapshot_error_names_the_method_and_points_at_planning() {
        let msg = snapshot_unsupported_error("get_code");
        assert!(msg.starts_with("get_code:"));
        assert!(msg.contains("snapshot_id is not supported"));
        assert!(msg.contains("close_loop.snapshot.md"));
    }

    // ---- Storage key padding semantics ---------------------------------
    //
    // The get_storage_at handler left-pads a short key to 32 bytes and
    // rejects keys longer than 32 bytes. Replicate the padding logic
    // exactly here so a regression that changes the padding direction
    // (left vs right) or the max length is caught.

    fn pad_storage_key(key: &[u8]) -> Result<[u8; 32], String> {
        if key.len() > 32 {
            return Err(format!(
                "storage key must be at most 32 bytes, got {}",
                key.len()
            ));
        }
        let mut padded = [0u8; 32];
        padded[32 - key.len()..].copy_from_slice(key);
        Ok(padded)
    }

    #[test]
    fn storage_key_zero_length_pads_to_zero_word() {
        assert_eq!(pad_storage_key(&[]).unwrap(), [0u8; 32]);
    }

    #[test]
    fn storage_key_32_bytes_is_identity() {
        let raw: [u8; 32] = [0x11; 32];
        assert_eq!(pad_storage_key(&raw).unwrap(), raw);
    }

    #[test]
    fn storage_key_33_bytes_is_rejected() {
        let raw = [0x22u8; 33];
        let err = pad_storage_key(&raw).unwrap_err();
        assert!(err.contains("at most 32"));
    }

    #[test]
    fn storage_key_is_left_padded_not_right_padded() {
        // A single byte 0xff must land in the last byte of the 32-byte
        // slot (index 31), NOT the first byte. This matches how the
        // get_storage_at handler builds the U256 slot index from the
        // raw request key.
        let padded = pad_storage_key(&[0xff]).unwrap();
        assert_eq!(padded[31], 0xff);
        assert_eq!(padded[..31], [0u8; 31]);
    }

    // ---- U256 balance round-trip ---------------------------------------
    //
    // The get_balance handler serializes the U256 balance as 32-byte BE.
    // This test locks that the serialization round-trips losslessly for
    // values near zero, in the middle of the range, and near the top.

    #[test]
    fn balance_round_trips_as_32_byte_be() {
        let cases: [revm_primitives::U256; 4] = [
            revm_primitives::U256::ZERO,
            revm_primitives::U256::from(1_000_000_000u64),
            revm_primitives::U256::MAX,
            revm_primitives::U256::from_be_bytes([0x7fu8; 32]),
        ];
        for value in cases {
            let bytes = value.to_be_bytes::<32>();
            assert_eq!(bytes.len(), 32);
            let round_trip = revm_primitives::U256::from_be_bytes::<32>(bytes);
            assert_eq!(round_trip, value);
        }
    }

    // ---- Handler-level tests -------------------------------------------
    //
    // These tests exercise the actual gRPC handler code for the four
    // read-path methods by building a real `BackendService` backed by a
    // `StorageModule` rooted in a temporary directory. They prove:
    //
    // 1. Non-empty `snapshot_id` is rejected with `success = false` and
    //    an error_message that mentions `close_loop.snapshot.md`.
    // 2. Malformed addresses produce `success = false` rather than
    //    silently returning default values.
    // 3. Reading an address that has never been written returns
    //    `found = false, success = true` (i.e. "no such account, no
    //    error either"), not a fake default.
    //
    // End-to-end "seed a real account and read it back" is still an
    // open follow-up under Section 2.4 (it requires exercising the
    // actual AccountCapsule serialization format, which lives inside
    // the execution crate). The tests below lock the invariants we
    // care about today — snapshot rejection, bad-address rejection,
    // and the not-found response shape.

    use tron_backend_common::ModuleManager;
    use tron_backend_common::StorageConfig as CommonStorageConfig;

    async fn build_read_path_test_service() -> (crate::BackendService, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let storage_config = CommonStorageConfig {
            data_dir: dir.path().to_string_lossy().to_string(),
            max_open_files: 100,
            cache_size: 8 * 1024 * 1024,
            write_buffer_size: 4 * 1024 * 1024,
            max_write_buffer_number: 2,
            compression: "lz4".to_string(),
        };

        let mut manager = ModuleManager::new();
        let storage_module =
            tron_backend_storage::StorageModule::new(&storage_config).expect("storage module");
        manager.register("storage", Box::new(storage_module));
        manager
            .init_all()
            .await
            .expect("manager init_all should bring up the storage engine");

        (crate::BackendService::new(manager), dir)
    }

    fn valid_21_byte_address() -> Vec<u8> {
        let mut buf = Vec::with_capacity(21);
        buf.push(0x41);
        buf.extend_from_slice(&[0x55u8; 20]);
        buf
    }

    #[tokio::test]
    async fn get_code_rejects_non_empty_snapshot_id() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let resp = service
            .get_code(tonic::Request::new(crate::backend::GetCodeRequest {
                address: valid_21_byte_address(),
                snapshot_id: "any-snapshot".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!resp.success);
        assert!(!resp.found);
        assert!(resp.error_message.contains("close_loop.snapshot.md"));
    }

    #[tokio::test]
    async fn get_nonce_rejects_non_empty_snapshot_id() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let resp = service
            .get_nonce(tonic::Request::new(crate::backend::GetNonceRequest {
                address: valid_21_byte_address(),
                snapshot_id: "x".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!resp.success);
        assert_eq!(resp.nonce, 0);
        assert!(resp.error_message.contains("close_loop.snapshot.md"));
    }

    #[tokio::test]
    async fn get_balance_rejects_non_empty_snapshot_id() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let resp = service
            .get_balance(tonic::Request::new(crate::backend::GetBalanceRequest {
                address: valid_21_byte_address(),
                snapshot_id: "x".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!resp.success);
        assert!(resp.balance.is_empty());
        assert!(resp.error_message.contains("close_loop.snapshot.md"));
    }

    #[tokio::test]
    async fn get_storage_at_rejects_non_empty_snapshot_id() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let resp = service
            .get_storage_at(tonic::Request::new(crate::backend::GetStorageAtRequest {
                address: valid_21_byte_address(),
                key: vec![0u8; 32],
                snapshot_id: "x".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!resp.success);
        assert!(resp.error_message.contains("close_loop.snapshot.md"));
    }

    #[tokio::test]
    async fn get_code_rejects_malformed_address() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let resp = service
            .get_code(tonic::Request::new(crate::backend::GetCodeRequest {
                address: vec![0x00u8; 7], // wrong length
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!resp.success);
        assert!(resp.error_message.contains("Invalid address"));
    }

    #[tokio::test]
    async fn get_storage_at_rejects_oversized_key() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let resp = service
            .get_storage_at(tonic::Request::new(crate::backend::GetStorageAtRequest {
                address: valid_21_byte_address(),
                key: vec![0u8; 33], // >32 bytes
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(!resp.success);
        assert!(resp.error_message.contains("at most 32 bytes"));
    }

    #[tokio::test]
    async fn get_nonce_unknown_address_returns_not_found_success() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let resp = service
            .get_nonce(tonic::Request::new(crate::backend::GetNonceRequest {
                address: valid_21_byte_address(),
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();

        // Reading an address that was never written is not an error:
        // the handler reports `found = false, success = true` so callers
        // can distinguish "missing account" from "transport failure".
        assert!(resp.success);
        assert!(!resp.found);
        assert_eq!(resp.nonce, 0);
        assert!(resp.error_message.is_empty());
    }

    #[tokio::test]
    async fn get_balance_unknown_address_returns_zeroed_balance() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let resp = service
            .get_balance(tonic::Request::new(crate::backend::GetBalanceRequest {
                address: valid_21_byte_address(),
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(resp.success);
        assert!(!resp.found);
        assert_eq!(resp.balance.len(), 32);
        assert!(resp.balance.iter().all(|b| *b == 0));
    }

    #[tokio::test]
    async fn get_code_unknown_address_returns_not_found_success() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let resp = service
            .get_code(tonic::Request::new(crate::backend::GetCodeRequest {
                address: valid_21_byte_address(),
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();

        assert!(resp.success);
        assert!(!resp.found);
        assert!(resp.code.is_empty());
    }

    #[tokio::test]
    async fn get_storage_at_unknown_slot_returns_zero_with_found_false() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let resp = service
            .get_storage_at(tonic::Request::new(crate::backend::GetStorageAtRequest {
                address: valid_21_byte_address(),
                key: vec![0u8; 32],
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();

        // An unwritten storage slot reads as U256::ZERO. The handler
        // returns success=true (no error) and found=false (distinguishing
        // "never written" from a genuine zero write).
        assert!(resp.success);
        assert!(!resp.found);
        assert_eq!(resp.value.len(), 32);
        assert!(resp.value.iter().all(|b| *b == 0));
    }

    // ---- iter 11: EVM snapshot handlers explicit-unsupported --------------
    //
    // Section 1.4 / 2.2 decision: `create_evm_snapshot` and
    // `revert_to_evm_snapshot` both return `success = false` with an
    // explicit error message pointing at `close_loop.snapshot.md`. The
    // iter 4 read-path tests covered the `snapshot_id` rejection on the
    // four read handlers; these two tests cover the execution-side
    // snapshot handlers that were closed in iter 4 as well but never
    // got targeted test coverage until now. Closes the partial flag on
    // Section 2.4 Rust-focused "Add negative tests for unsupported
    // snapshot/revert".

    #[tokio::test]
    async fn create_evm_snapshot_returns_explicit_unsupported() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let resp = service
            .create_evm_snapshot(tonic::Request::new(
                crate::backend::CreateEvmSnapshotRequest {},
            ))
            .await
            .unwrap()
            .into_inner();

        assert!(!resp.success);
        assert!(
            resp.snapshot_id.is_empty(),
            "Phase 1 must never hand back a synthetic snapshot id; \
             got: {:?}",
            resp.snapshot_id
        );
        assert!(
            resp.error_message.contains("close_loop.snapshot.md"),
            "error_message should cite the planning note; got: {}",
            resp.error_message
        );
        assert!(
            resp.error_message.contains("not supported"),
            "error_message should say 'not supported'; got: {}",
            resp.error_message
        );
    }

    #[tokio::test]
    async fn revert_to_evm_snapshot_returns_explicit_unsupported() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let resp = service
            .revert_to_evm_snapshot(tonic::Request::new(
                crate::backend::RevertToEvmSnapshotRequest {
                    snapshot_id: "any-snapshot-id".to_string(),
                },
            ))
            .await
            .unwrap()
            .into_inner();

        assert!(!resp.success);
        assert!(
            resp.error_message.contains("close_loop.snapshot.md"),
            "error_message should cite the planning note; got: {}",
            resp.error_message
        );
        assert!(
            resp.error_message.contains("not supported"),
            "error_message should say 'not supported'; got: {}",
            resp.error_message
        );
    }

    // ---- iter 11: put / delete / batch_write tx_id branching --------------
    //
    // Section 3.1 Rust-side handler branching was wired in iter 3: the
    // gRPC `put` / `delete` / `batch_write` handlers check
    // `req.transaction_id` and route to either the direct engine path
    // (empty string) or the per-tx buffered path (non-empty id via
    // `put_in_tx` / `delete_in_tx` / `batch_write_in_tx`). The storage
    // engine tests in iter 3 cover the engine layer directly; these
    // tests cover the gRPC handler layer that sits on top of it, which
    // was tracked as a "still open" follow-up under the Section 3.4
    // Java-focused "gRPC-handler coverage for transaction_id = ''"
    // item. Closing the Rust half of that item here; the Java half
    // stays open pending the gradle env unblock.
    //
    // The tests use the same `build_read_path_test_service()` helper as
    // the read-path tests — that helper registers a `StorageModule`
    // against a `TempDir`, so all storage writes go through the real
    // Rust storage engine with real RocksDB.

    #[tokio::test]
    async fn put_direct_path_round_trips_via_get() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        // Empty `transaction_id` must route through the direct engine
        // path. The write must be immediately visible via a direct
        // `get`, because the direct path bypasses the per-tx buffer.
        let put_resp = service
            .put(tonic::Request::new(crate::backend::PutRequest {
                database: "account".to_string(),
                key: b"direct-key".to_vec(),
                value: b"direct-val".to_vec(),
                transaction_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(put_resp.success, "direct put should succeed");
        assert!(put_resp.error_message.is_empty());

        let get_resp = service
            .get(tonic::Request::new(crate::backend::GetRequest {
                database: "account".to_string(),
                key: b"direct-key".to_vec(),
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(get_resp.success);
        assert!(get_resp.found);
        assert_eq!(get_resp.value, b"direct-val");
    }

    #[tokio::test]
    async fn put_buffered_path_is_invisible_until_commit() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        // Open a transaction against the "account" database.
        let begin_resp = service
            .begin_transaction(tonic::Request::new(
                crate::backend::BeginTransactionRequest {
                    database: "account".to_string(),
                },
            ))
            .await
            .unwrap()
            .into_inner();
        assert!(begin_resp.success);
        assert!(!begin_resp.transaction_id.is_empty());
        let tx_id = begin_resp.transaction_id;

        // Transactional put — routes through the per-tx buffer. Must
        // NOT be visible to a direct `get` before commit (the iter 3
        // Section 1.3 decision: no read-your-writes for transactional
        // put).
        let put_resp = service
            .put(tonic::Request::new(crate::backend::PutRequest {
                database: "account".to_string(),
                key: b"buffered-key".to_vec(),
                value: b"buffered-val".to_vec(),
                transaction_id: tx_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(put_resp.success);

        let pre_commit = service
            .get(tonic::Request::new(crate::backend::GetRequest {
                database: "account".to_string(),
                key: b"buffered-key".to_vec(),
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(pre_commit.success);
        assert!(
            !pre_commit.found,
            "buffered write must be invisible to direct get before commit"
        );

        // Commit the transaction; now the write becomes visible.
        let commit_resp = service
            .commit_transaction(tonic::Request::new(
                crate::backend::CommitTransactionRequest {
                    transaction_id: tx_id.clone(),
                },
            ))
            .await
            .unwrap()
            .into_inner();
        assert!(commit_resp.success);

        let post_commit = service
            .get(tonic::Request::new(crate::backend::GetRequest {
                database: "account".to_string(),
                key: b"buffered-key".to_vec(),
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(post_commit.success);
        assert!(post_commit.found);
        assert_eq!(post_commit.value, b"buffered-val");
    }

    #[tokio::test]
    async fn put_buffered_path_is_discarded_on_rollback() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let begin_resp = service
            .begin_transaction(tonic::Request::new(
                crate::backend::BeginTransactionRequest {
                    database: "account".to_string(),
                },
            ))
            .await
            .unwrap()
            .into_inner();
        assert!(begin_resp.success);
        let tx_id = begin_resp.transaction_id;

        service
            .put(tonic::Request::new(crate::backend::PutRequest {
                database: "account".to_string(),
                key: b"rollback-key".to_vec(),
                value: b"rollback-val".to_vec(),
                transaction_id: tx_id.clone(),
            }))
            .await
            .unwrap();

        // Rollback discards the buffer. A subsequent direct read must
        // report `found = false`, NOT return the would-be value.
        let rollback_resp = service
            .rollback_transaction(tonic::Request::new(
                crate::backend::RollbackTransactionRequest {
                    transaction_id: tx_id.clone(),
                },
            ))
            .await
            .unwrap()
            .into_inner();
        assert!(rollback_resp.success);

        let post_rollback = service
            .get(tonic::Request::new(crate::backend::GetRequest {
                database: "account".to_string(),
                key: b"rollback-key".to_vec(),
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(post_rollback.success);
        assert!(
            !post_rollback.found,
            "rolled-back buffer must leave no trace in the direct read"
        );
    }

    #[tokio::test]
    async fn put_unknown_transaction_id_is_rejected() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        // A non-empty transaction_id that was never opened must be
        // rejected with an explicit error — no silent fallback to the
        // direct path. The iter 3 Section 1.3 "no silent fallback"
        // decision is what this test enforces at the handler layer.
        let put_resp = service
            .put(tonic::Request::new(crate::backend::PutRequest {
                database: "account".to_string(),
                key: b"ghost-key".to_vec(),
                value: b"ghost-val".to_vec(),
                transaction_id: "tx-does-not-exist".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!put_resp.success);
        assert!(
            put_resp.error_message.contains("not found")
                || put_resp.error_message.contains("transaction"),
            "unknown tx_id should surface an explicit 'not found'/'transaction' error; got: {}",
            put_resp.error_message
        );

        // Confirm the write did NOT leak into the direct path either.
        let get_resp = service
            .get(tonic::Request::new(crate::backend::GetRequest {
                database: "account".to_string(),
                key: b"ghost-key".to_vec(),
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(get_resp.success);
        assert!(!get_resp.found);
    }

    #[tokio::test]
    async fn delete_direct_path_removes_existing_key() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        // Seed a value via the direct put path.
        service
            .put(tonic::Request::new(crate::backend::PutRequest {
                database: "account".to_string(),
                key: b"delete-key".to_vec(),
                value: b"delete-val".to_vec(),
                transaction_id: String::new(),
            }))
            .await
            .unwrap();

        // Direct delete must remove it immediately.
        let delete_resp = service
            .delete(tonic::Request::new(crate::backend::DeleteRequest {
                database: "account".to_string(),
                key: b"delete-key".to_vec(),
                transaction_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(delete_resp.success);

        let get_resp = service
            .get(tonic::Request::new(crate::backend::GetRequest {
                database: "account".to_string(),
                key: b"delete-key".to_vec(),
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(get_resp.success);
        assert!(!get_resp.found);
    }

    #[tokio::test]
    async fn delete_buffered_path_honors_rollback() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        // Seed a value via direct put so there is something to delete.
        service
            .put(tonic::Request::new(crate::backend::PutRequest {
                database: "account".to_string(),
                key: b"tx-delete-key".to_vec(),
                value: b"tx-delete-val".to_vec(),
                transaction_id: String::new(),
            }))
            .await
            .unwrap();

        let begin_resp = service
            .begin_transaction(tonic::Request::new(
                crate::backend::BeginTransactionRequest {
                    database: "account".to_string(),
                },
            ))
            .await
            .unwrap()
            .into_inner();
        let tx_id = begin_resp.transaction_id;

        // Transactional delete: buffered, so the direct read still
        // sees the value before commit.
        service
            .delete(tonic::Request::new(crate::backend::DeleteRequest {
                database: "account".to_string(),
                key: b"tx-delete-key".to_vec(),
                transaction_id: tx_id.clone(),
            }))
            .await
            .unwrap();

        let pre_commit = service
            .get(tonic::Request::new(crate::backend::GetRequest {
                database: "account".to_string(),
                key: b"tx-delete-key".to_vec(),
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(pre_commit.found);
        assert_eq!(pre_commit.value, b"tx-delete-val");

        // Rollback discards the pending delete; value still exists.
        service
            .rollback_transaction(tonic::Request::new(
                crate::backend::RollbackTransactionRequest {
                    transaction_id: tx_id.clone(),
                },
            ))
            .await
            .unwrap();

        let post_rollback = service
            .get(tonic::Request::new(crate::backend::GetRequest {
                database: "account".to_string(),
                key: b"tx-delete-key".to_vec(),
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(post_rollback.found);
        assert_eq!(post_rollback.value, b"tx-delete-val");
    }

    #[tokio::test]
    async fn batch_write_direct_path_applies_all_ops() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        // Seed one value so we can verify batch delete alongside batch put.
        service
            .put(tonic::Request::new(crate::backend::PutRequest {
                database: "account".to_string(),
                key: b"old".to_vec(),
                value: b"x".to_vec(),
                transaction_id: String::new(),
            }))
            .await
            .unwrap();

        let ops = vec![
            crate::backend::WriteOperation {
                r#type: 0, // PUT
                key: b"batch-a".to_vec(),
                value: b"1".to_vec(),
            },
            crate::backend::WriteOperation {
                r#type: 0, // PUT
                key: b"batch-b".to_vec(),
                value: b"2".to_vec(),
            },
            crate::backend::WriteOperation {
                r#type: 1, // DELETE
                key: b"old".to_vec(),
                value: Vec::new(),
            },
        ];
        let resp = service
            .batch_write(tonic::Request::new(crate::backend::BatchWriteRequest {
                database: "account".to_string(),
                operations: ops,
                transaction_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success);
        // Direct-path operations_applied must equal the ops count
        // (the iter 6 fix: transactional branch returns 0, direct
        // branch returns ops.len()).
        assert_eq!(resp.operations_applied, 3);

        // Verify each op.
        for (k, expected_found, expected_val) in [
            (b"batch-a".to_vec(), true, b"1".to_vec()),
            (b"batch-b".to_vec(), true, b"2".to_vec()),
            (b"old".to_vec(), false, Vec::new()),
        ] {
            let r = service
                .get(tonic::Request::new(crate::backend::GetRequest {
                    database: "account".to_string(),
                    key: k,
                    snapshot_id: String::new(),
                }))
                .await
                .unwrap()
                .into_inner();
            assert!(r.success);
            assert_eq!(r.found, expected_found);
            if expected_found {
                assert_eq!(r.value, expected_val);
            }
        }
    }

    #[tokio::test]
    async fn batch_write_buffered_path_reports_zero_operations_applied() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let begin_resp = service
            .begin_transaction(tonic::Request::new(
                crate::backend::BeginTransactionRequest {
                    database: "account".to_string(),
                },
            ))
            .await
            .unwrap()
            .into_inner();
        let tx_id = begin_resp.transaction_id;

        let ops = vec![
            crate::backend::WriteOperation {
                r#type: 0,
                key: b"bx-a".to_vec(),
                value: b"1".to_vec(),
            },
            crate::backend::WriteOperation {
                r#type: 0,
                key: b"bx-b".to_vec(),
                value: b"2".to_vec(),
            },
        ];
        let resp = service
            .batch_write(tonic::Request::new(crate::backend::BatchWriteRequest {
                database: "account".to_string(),
                operations: ops,
                transaction_id: tx_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(resp.success);
        // iter 6 fix: buffered branch must report 0 — a buffer append
        // is not the same as a persisted commit.
        assert_eq!(
            resp.operations_applied, 0,
            "transactional batch_write must NOT return ops.len() as operations_applied; \
             that would let a caller mistake buffering for commit"
        );

        // Both writes must be invisible to direct read before commit.
        for k in [b"bx-a".to_vec(), b"bx-b".to_vec()] {
            let r = service
                .get(tonic::Request::new(crate::backend::GetRequest {
                    database: "account".to_string(),
                    key: k,
                    snapshot_id: String::new(),
                }))
                .await
                .unwrap()
                .into_inner();
            assert!(!r.found);
        }

        // Commit, then they become visible.
        service
            .commit_transaction(tonic::Request::new(
                crate::backend::CommitTransactionRequest {
                    transaction_id: tx_id.clone(),
                },
            ))
            .await
            .unwrap();
        for (k, expected) in [
            (b"bx-a".to_vec(), b"1".to_vec()),
            (b"bx-b".to_vec(), b"2".to_vec()),
        ] {
            let r = service
                .get(tonic::Request::new(crate::backend::GetRequest {
                    database: "account".to_string(),
                    key: k,
                    snapshot_id: String::new(),
                }))
                .await
                .unwrap()
                .into_inner();
            assert!(r.found);
            assert_eq!(r.value, expected);
        }
    }

    #[tokio::test]
    async fn batch_write_unknown_transaction_id_is_rejected() {
        use crate::backend::backend_server::Backend;
        let (service, _dir) = build_read_path_test_service().await;

        let resp = service
            .batch_write(tonic::Request::new(crate::backend::BatchWriteRequest {
                database: "account".to_string(),
                operations: vec![crate::backend::WriteOperation {
                    r#type: 0,
                    key: b"k".to_vec(),
                    value: b"v".to_vec(),
                }],
                transaction_id: "tx-not-opened".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!resp.success);
        assert_eq!(resp.operations_applied, 0);
        assert!(
            resp.error_message.contains("not found")
                || resp.error_message.contains("transaction"),
            "unknown tx_id on batch_write should surface an explicit error; got: {}",
            resp.error_message
        );

        let get_resp = service
            .get(tonic::Request::new(crate::backend::GetRequest {
                database: "account".to_string(),
                key: b"k".to_vec(),
                snapshot_id: String::new(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!get_resp.found);
    }
}

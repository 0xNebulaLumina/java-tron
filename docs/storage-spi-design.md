# Storage SPI Design Document

## Overview

This document defines the Storage Service Provider Interface (SPI) for replacing the current embedded LevelDB/RocksDB implementation in java-tron with a Rust-based multi-process database service.

## Current Storage Architecture Analysis

### Key Components

1. **DbSourceInter<V>** - Core storage interface
2. **BatchSourceInter<K,V>** - Batch operations interface  
3. **SourceInter<K,V>** - Basic CRUD operations
4. **DB<K,V>** - Abstract database interface
5. **IRevokingDB** - Transaction/snapshot management
6. **TronDatabase<T>** - Base database class
7. **TronStoreWithRevoking<T>** - Store with transaction support

### Current Operations Inventory

#### Basic Operations
- `get(key)` - Single key retrieval
- `put(key, value)` - Single key-value insertion
- `delete(key)` - Key deletion
- `has(key)` - Key existence check

#### Batch Operations
- `updateByBatch(Map<K,V> rows)` - Batch write with default options
- `updateByBatch(Map<K,V> rows, WriteOptionsWrapper options)` - Batch write with custom options

#### Iterator Operations
- `iterator()` - Full database iteration
- `getKeysNext(key, limit)` - Get next N keys from position
- `getValuesNext(key, limit)` - Get next N values from position
- `getNext(key, limit)` - Get next N key-value pairs
- `prefixQuery(prefix)` - Prefix-based query

#### Database Management
- `initDB()` - Database initialization
- `closeDB()` - Database cleanup
- `resetDb()` - Database reset/clear
- `isAlive()` - Health check
- `size()` - Get total records count
- `isEmpty()` - Check if database is empty

#### Transaction/Snapshot Support
- `buildSession()` - Create transaction session
- `commit()` - Commit transaction
- `revoke()` - Rollback transaction
- `merge()` - Merge snapshots
- `setCursor(cursor)` - Set read cursor position

#### Metadata Operations
- `getDBName()` - Get database name
- `stat()` - Get database statistics
- `allKeys()` - Get all keys
- `allValues()` - Get all values
- `getTotal()` - Get total record count

## Proposed Storage SPI

### Core Interface Definition

```java
public interface StorageSPI {
    // Basic Operations
    CompletableFuture<byte[]> get(String dbName, byte[] key);
    CompletableFuture<Void> put(String dbName, byte[] key, byte[] value);
    CompletableFuture<Void> delete(String dbName, byte[] key);
    CompletableFuture<Boolean> has(String dbName, byte[] key);
    
    // Batch Operations
    CompletableFuture<Void> batchWrite(String dbName, Map<byte[], byte[]> operations);
    CompletableFuture<Map<byte[], byte[]>> batchGet(String dbName, List<byte[]> keys);
    
    // Iterator Operations
    CompletableFuture<StorageIterator> iterator(String dbName);
    CompletableFuture<StorageIterator> iterator(String dbName, byte[] startKey);
    CompletableFuture<List<byte[]>> getKeysNext(String dbName, byte[] startKey, int limit);
    CompletableFuture<List<byte[]>> getValuesNext(String dbName, byte[] startKey, int limit);
    CompletableFuture<Map<byte[], byte[]>> getNext(String dbName, byte[] startKey, int limit);
    CompletableFuture<Map<byte[], byte[]>> prefixQuery(String dbName, byte[] prefix);
    
    // Database Management
    CompletableFuture<Void> initDB(String dbName, StorageConfig config);
    CompletableFuture<Void> closeDB(String dbName);
    CompletableFuture<Void> resetDB(String dbName);
    CompletableFuture<Boolean> isAlive(String dbName);
    CompletableFuture<Long> size(String dbName);
    CompletableFuture<Boolean> isEmpty(String dbName);
    
    // Transaction Support
    CompletableFuture<String> beginTransaction(String dbName);
    CompletableFuture<Void> commitTransaction(String transactionId);
    CompletableFuture<Void> rollbackTransaction(String transactionId);
    
    // Snapshot Support
    CompletableFuture<String> createSnapshot(String dbName);
    CompletableFuture<Void> deleteSnapshot(String snapshotId);
    CompletableFuture<byte[]> getFromSnapshot(String snapshotId, byte[] key);
    
    // Metadata
    CompletableFuture<StorageStats> getStats(String dbName);
    CompletableFuture<List<String>> listDatabases();
    
    // Health & Monitoring
    CompletableFuture<HealthStatus> healthCheck();
    void registerMetricsCallback(MetricsCallback callback);
}
```

### Supporting Classes

```java
public class StorageConfig {
    private String engine; // "ROCKSDB" or "LEVELDB"
    private Map<String, Object> engineOptions;
    private boolean enableStatistics;
    private int maxOpenFiles;
    private long blockCacheSize;
    // ... other configuration options
}

public class StorageStats {
    private long totalKeys;
    private long totalSize;
    private Map<String, String> engineStats;
    private long lastModified;
}

public interface StorageIterator extends AutoCloseable {
    CompletableFuture<Boolean> hasNext();
    CompletableFuture<Map.Entry<byte[], byte[]>> next();
    CompletableFuture<Void> seek(byte[] key);
    CompletableFuture<Void> seekToFirst();
    CompletableFuture<Void> seekToLast();
}

public enum HealthStatus {
    HEALTHY, DEGRADED, UNHEALTHY
}

public interface MetricsCallback {
    void onMetrics(String dbName, Map<String, Object> metrics);
}
```

## gRPC Protocol Definition

```protobuf
syntax = "proto3";

service StorageService {
    // Basic Operations
    rpc Get(GetRequest) returns (GetResponse);
    rpc Put(PutRequest) returns (PutResponse);
    rpc Delete(DeleteRequest) returns (DeleteResponse);
    rpc Has(HasRequest) returns (HasResponse);
    
    // Batch Operations
    rpc BatchWrite(BatchWriteRequest) returns (BatchWriteResponse);
    rpc BatchGet(BatchGetRequest) returns (BatchGetResponse);
    
    // Iterator Operations
    rpc Iterator(IteratorRequest) returns (stream IteratorResponse);
    rpc GetKeysNext(GetKeysNextRequest) returns (GetKeysNextResponse);
    rpc GetValuesNext(GetValuesNextRequest) returns (GetValuesNextResponse);
    rpc GetNext(GetNextRequest) returns (GetNextResponse);
    rpc PrefixQuery(PrefixQueryRequest) returns (PrefixQueryResponse);
    
    // Database Management
    rpc InitDB(InitDBRequest) returns (InitDBResponse);
    rpc CloseDB(CloseDBRequest) returns (CloseDBResponse);
    rpc ResetDB(ResetDBRequest) returns (ResetDBResponse);
    rpc IsAlive(IsAliveRequest) returns (IsAliveResponse);
    rpc Size(SizeRequest) returns (SizeResponse);
    rpc IsEmpty(IsEmptyRequest) returns (IsEmptyResponse);
    
    // Transaction Support
    rpc BeginTransaction(BeginTransactionRequest) returns (BeginTransactionResponse);
    rpc CommitTransaction(CommitTransactionRequest) returns (CommitTransactionResponse);
    rpc RollbackTransaction(RollbackTransactionRequest) returns (RollbackTransactionResponse);
    
    // Snapshot Support
    rpc CreateSnapshot(CreateSnapshotRequest) returns (CreateSnapshotResponse);
    rpc DeleteSnapshot(DeleteSnapshotRequest) returns (DeleteSnapshotResponse);
    rpc GetFromSnapshot(GetFromSnapshotRequest) returns (GetFromSnapshotResponse);
    
    // Metadata
    rpc GetStats(GetStatsRequest) returns (GetStatsResponse);
    rpc ListDatabases(ListDatabasesRequest) returns (ListDatabasesResponse);
    
    // Health & Monitoring
    rpc HealthCheck(HealthCheckRequest) returns (HealthCheckResponse);
    rpc StreamMetrics(StreamMetricsRequest) returns (stream MetricsResponse);
}

message GetRequest {
    string db_name = 1;
    bytes key = 2;
}

message GetResponse {
    bytes value = 1;
    bool found = 2;
}

message BatchWriteRequest {
    string db_name = 1;
    repeated BatchOperation operations = 2;
}

message BatchOperation {
    enum Type {
        PUT = 0;
        DELETE = 1;
    }
    Type type = 1;
    bytes key = 2;
    bytes value = 3; // only for PUT operations
}

// ... other message definitions
```

## Implementation Strategy

### Phase 1: SPI Abstraction Layer
1. Create `StorageSPI` interface and implementations
2. Create `StorageAdapter` to wrap existing RocksDB/LevelDB
3. Update all `TronDatabase` and `TronStoreWithRevoking` classes to use SPI
4. Ensure backward compatibility with existing functionality

### Phase 2: gRPC Client Implementation
1. Implement `GrpcStorageSPI` that communicates with Rust service
2. Add connection pooling and retry logic
3. Implement async/sync operation mapping
4. Add comprehensive error handling and circuit breaker

### Phase 3: Rust Storage Service
1. Implement gRPC server in Rust
2. Add RocksDB/LevelDB backends using rust crates
3. Implement transaction and snapshot management
4. Add metrics and monitoring endpoints

### Phase 4: Integration & Testing
1. Create comprehensive test suite
2. Performance benchmarking
3. Failover and recovery testing
4. Data migration tools

## Migration Path

### Configuration-Based Switching
```java
// In application.conf
storage {
    provider = "EMBEDDED" // or "GRPC"
    grpc {
        host = "localhost"
        port = 50011
        connection_pool_size = 10
        timeout_ms = 5000
    }
}
```

### Gradual Migration
1. Start with read-only operations via gRPC
2. Gradually move write operations
3. Implement dual-write for verification
4. Full cutover after validation

## Error Handling Strategy

### Retry Logic
- Exponential backoff for transient failures
- Circuit breaker for persistent failures
- Fallback to local cache when possible

### Consistency Guarantees
- Transaction isolation maintained through snapshot IDs
- Batch operations are atomic
- Proper cleanup on connection failures

## Performance Considerations

### Optimization Techniques
1. **Connection Pooling**: Maintain persistent gRPC connections
2. **Batch Optimization**: Combine small operations into batches
3. **Streaming**: Use gRPC streaming for large result sets
4. **Caching**: Local caching for frequently accessed data
5. **Compression**: Enable gRPC compression for large payloads

### Monitoring Metrics
- Request latency (p50, p95, p99)
- Throughput (ops/sec)
- Error rates
- Connection pool utilization
- Cache hit rates

## Security Considerations

### Authentication & Authorization
- mTLS for service-to-service communication
- API key authentication for additional security
- Role-based access control for different operations

### Data Protection
- Encryption in transit (TLS)
- Encryption at rest (configurable)
- Audit logging for sensitive operations

## Deployment Architecture

```
┌─────────────────────┐         ┌──────────────────────┐
│   Java Execution    │  gRPC   │   Rust DB Service    │
│   + Network Node    │◄──────► │                      │
│                     │         │  ┌─────────────────┐ │
│  ┌──────────────┐   │         │  │   RocksDB       │ │
│  │ StorageSPI   │   │         │  │   Engine        │ │
│  │ GrpcClient   │   │         │  └─────────────────┘ │
│  └──────────────┘   │         │                      │
└─────────────────────┘         │  ┌─────────────────┐ │
                                │  │   Metrics &     │ │
                                │  │   Monitoring    │ │
                                │  └─────────────────┘ │
                                └──────────────────────┘
```

## Implementation Progress

### ✅ Completed Phases

1. **✅ Storage SPI Design & Documentation**
   - Complete interface definition with 20+ methods
   - Comprehensive gRPC protocol specification
   - Architecture and migration strategy documented

2. **✅ StorageSPI Interface & Supporting Classes**
   - `StorageSPI` interface implemented
   - `StorageConfig`, `StorageStats`, `HealthStatus` classes created
   - `StorageIterator` interface with async support
   - `MetricsCallback` interface for monitoring

3. **✅ gRPC Protocol Implementation**
   - Complete protobuf definition (300+ lines) 
   - Java stub generation configured in Gradle
   - All message types and service methods defined

4. **✅ Rust Storage Service**
   - Full gRPC server implementation with all RPC methods
   - RocksDB integration with configurable options
   - Transaction and snapshot management
   - Error handling and logging throughout

5. **✅ Java gRPC Client (GrpcStorageSPI)**
   - **Real gRPC communication** for all 20+ methods
   - Proper protobuf message handling and type conversion
   - Error mapping from `StatusRuntimeException` to `RuntimeException`
   - Async operations using `CompletableFuture` with blocking stubs
   - Resource management with proper channel cleanup

6. **✅ Development Infrastructure**
   - Docker Compose orchestration for multi-service testing
   - Makefile automation for build and test workflows
   - gRPC and protobuf dependencies configured in Gradle
   - Protobuf code generation working correctly

### 🔄 Current Phase: Performance Validation

7. **[ ] Integration Testing & Performance Benchmarking**
   - End-to-end testing with running Rust gRPC server
   - Performance comparison against embedded storage
   - Load testing with realistic java-tron workloads
   - Latency analysis for different operation types

### 📋 Remaining Tasks

8. **[ ] Production Readiness**
   - Connection pooling and retry logic enhancement
   - Comprehensive monitoring and metrics integration
   - Security implementation (mTLS, authentication)
   - Advanced error handling and circuit breaker patterns

9. **[ ] Migration & Deployment**
   - Configuration-based storage provider switching
   - Data migration tools and procedures
   - Gradual rollout strategy with feature flags
   - Production deployment and monitoring setup

## Current Status

**The PoC implementation is COMPLETE and ready for performance testing.** All core components have been implemented with real gRPC communication between Java and Rust layers. The next critical milestone is validating performance characteristics against the existing embedded storage solution. 
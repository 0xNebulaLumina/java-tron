# Tron Storage Service (Rust)

A high-performance gRPC-based storage service for TRON blockchain, written in Rust using RocksDB.

## Features

- **gRPC API**: High-performance protocol buffer-based communication
- **RocksDB Backend**: Optimized for blockchain workloads
- **Transaction Support**: ACID transactions with rollback capability
- **Snapshot Management**: Point-in-time snapshots for consistent reads
- **Metrics & Monitoring**: Prometheus-compatible metrics endpoint
- **Health Checks**: Built-in health check endpoints
- **Configuration**: Flexible TOML-based configuration

## Quick Start

### Prerequisites

- Rust 1.75 or later
- Protocol Buffers compiler (`protoc`)

### Building

```bash
# Clone the repository
cd rust-storage-service

# Build the service
cargo build --release

# Run the service
cargo run
```

### Using Docker

```bash
# Build the Docker image
docker build -t tron-storage-service .

# Run the container
docker run -p 50051:50051 -p 9090:9090 tron-storage-service
```

### Using Docker Compose

```bash
# From the project root
docker-compose up rust-storage
```

## Configuration

The service can be configured using:

1. **Configuration file** (`config.toml`)
2. **Environment variables** (prefixed with `TRON_STORAGE_`)
3. **Command line arguments**

### Configuration File Example

```toml
# Server configuration
host = "127.0.0.1"
port = 50051
data_dir = "./data"
default_engine = "ROCKSDB"

# RocksDB configuration
[rocksdb]
max_open_files = 5000
block_cache_size = 1073741824  # 1GB
enable_statistics = true
level_compaction_dynamic_level_bytes = true
max_background_compactions = 4
target_file_size_base = 67108864      # 64MB
max_bytes_for_level_base = 536870912  # 512MB

# Metrics configuration
[metrics]
enabled = true
port = 9090
path = "/metrics"
```

### Environment Variables

- `TRON_STORAGE_HOST`: Server host (default: 127.0.0.1)
- `TRON_STORAGE_PORT`: Server port (default: 50051)
- `TRON_STORAGE_DATA_DIR`: Data directory (default: ./data)
- `TRON_STORAGE_DEFAULT_ENGINE`: Database engine (default: ROCKSDB)
- `RUST_LOG`: Log level (debug, info, warn, error)

## API Documentation

The service exposes a gRPC API defined in `proto/storage.proto`. Key operations include:

### Basic Operations
- `Get(key)` - Retrieve a value by key
- `Put(key, value)` - Store a key-value pair
- `Delete(key)` - Delete a key
- `Has(key)` - Check if key exists

### Batch Operations
- `BatchWrite(operations)` - Execute multiple write operations atomically
- `BatchGet(keys)` - Retrieve multiple values

### Iterator Operations
- `Iterator()` - Stream all key-value pairs
- `GetKeysNext(start_key, limit)` - Get next N keys
- `GetValuesNext(start_key, limit)` - Get next N values
- `PrefixQuery(prefix)` - Query by key prefix

### Database Management
- `InitDB(db_name, config)` - Initialize a database
- `CloseDB(db_name)` - Close a database
- `ResetDB(db_name)` - Reset/clear a database
- `Size(db_name)` - Get record count
- `GetStats(db_name)` - Get database statistics

### Transaction Support
- `BeginTransaction(db_name)` - Start a transaction
- `CommitTransaction(transaction_id)` - Commit a transaction
- `RollbackTransaction(transaction_id)` - Rollback a transaction

### Snapshot Support
- `CreateSnapshot(db_name)` - Create a point-in-time snapshot
- `DeleteSnapshot(snapshot_id)` - Delete a snapshot
- `GetFromSnapshot(snapshot_id, key)` - Read from snapshot

## Monitoring

### Health Checks

The service provides health check endpoints:

```bash
# gRPC health check
grpc_health_probe -addr=localhost:50051

# HTTP health check (if metrics enabled)
curl http://localhost:9090/health
```

### Metrics

Prometheus-compatible metrics are available at `/metrics` endpoint:

```bash
curl http://localhost:9090/metrics
```

Key metrics include:
- Request latency histograms
- Request rate counters
- Database size gauges
- Error rate counters
- Connection pool metrics

## Performance Tuning

### RocksDB Configuration

Key parameters for performance tuning:

- `max_open_files`: Maximum number of open files (default: 5000)
- `block_cache_size`: Block cache size in bytes (default: 1GB)
- `max_background_compactions`: Background compaction threads (default: 4)
- `target_file_size_base`: Target file size for L1 (default: 64MB)
- `max_bytes_for_level_base`: Maximum bytes for level base (default: 512MB)

### Memory Usage

- Block cache: Shared across all databases
- Write buffers: Per-database memory for writes
- Bloom filters: Reduce disk reads for non-existent keys

### Disk Usage

- Data is stored in `data_dir` with subdirectories per database
- Regular compaction reduces space amplification
- Snapshots share data with main database (copy-on-write)

## Troubleshooting

### Common Issues

1. **Port already in use**: Change the port in configuration
2. **Permission denied**: Ensure write access to data directory
3. **Out of memory**: Reduce block_cache_size or max_open_files
4. **Slow performance**: Check disk I/O and adjust compaction settings

### Logging

Set `RUST_LOG=debug` for detailed logging:

```bash
RUST_LOG=debug cargo run
```

### Database Corruption

If database corruption occurs:

1. Stop the service
2. Remove the corrupted database directory
3. Restore from backup or resync from network

## Development

### Building from Source

```bash
# Install dependencies
sudo apt-get install pkg-config libssl-dev libclang-dev

# Build
cargo build

# Run tests
cargo test

# Run with debug logging
RUST_LOG=debug cargo run
```

### Protocol Buffer Changes

After modifying `proto/storage.proto`:

```bash
# Regenerate Rust code
cargo build
```

The build script automatically regenerates the gRPC code.

## License

This project is licensed under the same license as the TRON project. 
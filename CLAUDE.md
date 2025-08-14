# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build System and Commands

### Primary Build Commands
- **Build project**: `./gradlew clean build -x test` (excludes tests for faster builds)
- **Build with tests**: `./gradlew build`
- **Clean build**: `./gradlew clean`
- **Build specific module**: `./gradlew :framework:build` (replace framework with module name)

### Testing Commands
- **Run all tests**: `./gradlew test`
- **Run specific test**: `./gradlew :framework:test --tests "TestClassName"`
- **Skip dependency verification**: Add `--dependency-verification=off` to any gradlew command

### Makefile Commands (Storage PoC)
- **Build all components**: `make build` (builds both Rust and Java)
- **Run Java tests**: `make java-test`
- **Run performance tests**: `make performance-test`
- **Run integration tests**: `make integration-test`
- **Run Tron workload tests**: `make tron-workload-test`
- **Clean artifacts**: `make clean`

### Docker Commands
- **Build Docker image**: `docker build -t tronprotocol/java-tron .`
- **Run container**: `docker run -it -d -p 8090:8090 -p 18888:18888 -p 50051:50051 tronprotocol/java-tron`

## Code Architecture

### Migration Strategy: Java → Rust
This codebase is undergoing a **gradual migration from Java to Rust** for performance and reliability improvements. The strategy involves:
- **Phase 1**: Move storage logic to Rust service (rust-backend/)
- **Phase 2**: Move execution logic to Rust service
- **Future**: Continue migrating performance-critical components to Rust

### Module Structure
- **framework**: Core node implementation, SPI abstractions, networking, consensus
- **protocol**: Protocol buffer definitions for Java-Rust communication
- **actuator**: Transaction processing (being migrated to Rust)
- **consensus**: Consensus mechanism implementation (PBFT, witness)
- **chainbase**: Legacy database abstraction (being replaced by Rust storage)
- **common**: Shared utilities and common components
- **crypto**: Cryptographic functions and key management
- **rust-backend/**: **New Rust service** containing migrated storage and execution logic

### Current Architecture: Hybrid Java-Rust System

#### Storage Migration (In Progress)
- **Legacy Path**: Java → chainbase → RocksDB (being phased out)
- **New Path**: Java → gRPC → Rust storage service → RocksDB
- **Storage SPI**: Abstraction layer in framework/src/main/java/org/tron/core/storage/spi/
- **Dual Mode**: StorageSpiFactory.java switches between embedded (legacy) and remote (Rust)
- **Rust Service**: rust-backend/ contains the new high-performance storage implementation

#### Execution Migration (In Progress)
- **Legacy Path**: Java actuators and execution engine
- **New Path**: Java → gRPC → Rust execution service
- **Execution SPI**: Abstraction in framework/src/main/java/org/tron/core/execution/spi/
- **State Sync**: RuntimeSpiImpl.java handles state changes between Java and Rust
- **Migration Status**: Storage moved, execution logic actively being migrated

#### Network Layer
- **P2P Network**: framework/src/main/java/org/tron/core/net/ handles peer-to-peer communication
- **Message Handling**: TronMessage hierarchy for different message types
- **Service Layer**: RPC and HTTP API services in framework/src/main/java/org/tron/core/services/

### Configuration Files
- **Main config**: `main_net_config_remote.conf` for mainnet configuration
- **Test configs**: Various test network configurations available
- **Docker config**: `docker-compose.yml` for containerized deployment

## Development Guidelines

### How to run
- **Run Rust Service**: `cd rust-backend && cargo run --release`
- **Run Java Service**:
  - run rust service first
  - then build the java service by `./gradlew clean build -x test --dependency-verification=off`
  - then run the java service by:
```
nohup java -Xms9G -Xmx9G -XX:ReservedCodeCacheSize=256m \
     -XX:MetaspaceSize=256m -XX:MaxMetaspaceSize=512m \
     -XX:MaxDirectMemorySize=1G -XX:+PrintGCDetails \
     -XX:+PrintGCDateStamps  -Xloggc:gc.log \
     -XX:+UseConcMarkSweepGC -XX:NewRatio=2 \
     -XX:+CMSScavengeBeforeRemark -XX:+ParallelRefProcEnabled \
     -XX:+HeapDumpOnOutOfMemoryError \
     -XX:+UseCMSInitiatingOccupancyOnly  -XX:CMSInitiatingOccupancyFraction=70 \
     -jar ./build/libs/FullNode.jar -c ./main_net_config_remote.conf \
     --execution-spi-enabled --execution-mode "REMOTE" >> start.log 2>&1 &
```
  - the java service logs then can be found in `logs/tron.log`

### Java Requirements
- **JDK Version**: Oracle JDK 1.8 (JDK 1.9+ not supported)
- **Encoding**: UTF-8 for all source files
- **Code Style**: Google Java Style Guide compliance required

### Rust Migration Components
- **rust-backend/**: Main Rust service containing migrated storage and execution logic
  - **Storage Implementation**: High-performance RocksDB operations
  - **Execution Engine**: Smart contract and transaction execution
  - **gRPC Server**: Serves requests from Java frontend
  - **Build**: `cd rust-backend && cargo build --release`
- **state-digest-jni/**: Native library for state digest computations
- **Proto Definitions**: Shared protobuf schemas for Java-Rust communication

### Branch Strategy
- **develop**: Main development branch for new features
- **master**: Stable release branch
- **feature/**: Feature branches pulled from develop
- **hotfix/**: Bug fix branches pulled from master
- **release/**: Release preparation branches

### Commit Guidelines
Format: `<type>(<scope>): <subject>`
- Types: feat, fix, docs, style, refactor, test, chore
- Subject: Present tense, lowercase, no period, <50 characters
- Example: `feat(storage): implement dual storage mode`

### Testing Strategy
- **Unit Tests**: Use JUnit 4.13.2, located in src/test/ directories
- **Integration Tests**: Cross-module testing with real components
- **Performance Tests**: Storage and execution benchmarks in Makefile
- **Blockchain Workload Tests**: Comprehensive Tron-specific testing scenarios

## Performance and Optimization

### Current Performance Baseline
- **Single Operations**: 666-1,193 ops/sec (production-ready)
- **Batch Operations**: Up to 88K ops/sec
- **Block Processing**: 62,000 tx/sec (exceeds mainnet requirements)
- **Memory Usage**: ~218MB baseline (efficient)

### Performance Testing (Java vs Rust Migration)
- **Migration Comparison**: Use `make dual-mode-perf` to compare legacy Java vs new Rust performance
- **Rust Storage**: Use `make remote-tron-workload` to test Rust storage service
- **Legacy Java**: Use `make embedded-tron-workload` to test legacy Java storage
- **Comprehensive Testing**: Use `make tron-workload-test` for full Java+Rust system testing
- **Storage Benchmarks**: Located in framework storage SPI tests
- **Reports**: Generated in `reports/` directory with timestamps showing migration benefits

## Special Considerations

### Dependency Management
- **Gradle Verification**: May need `--dependency-verification=off` flag
- **Update Verification**: Use `make update-verification` to fix metadata
- **JitPack Dependencies**: Some dependencies from jitpack.io

### Security Requirements
- **No API Keys**: Never commit sensitive information
- **Private Keys**: Use localwitness configuration for witnesses
- **Network Security**: Proper port configuration for P2P networking

### Deployment Modes
- **Full Node**: Standard blockchain node with complete data
- **Super Representative**: Witness node for block production (requires --witness flag)
- **Solidity Node**: Read-only node for solidified blocks
- **Event Server**: Event handling and filtering capabilities

### Database and Storage (Migration in Progress)
- **Legacy Storage**: RocksDB via chainbase abstraction (being phased out)
- **New Storage**: RocksDB via Rust storage service (rust-backend/)
- **Migration Mode**: Dual storage support via StorageSpiFactory configuration
- **Data Directory**: Configurable via `-d` flag or configuration
- **Backup**: BackupManager in framework/src/main/java/org/tron/common/backup/
- **State Sync**: RuntimeSpiImpl.java handles synchronization between Java and Rust state

## Integration Points

### gRPC Services (Java-Rust Communication)
- **Rust Storage Service**: Primary storage via gRPC (port 50011 default)
- **Rust Execution Service**: Transaction execution via gRPC (being migrated)
- **Java Frontend**: API layer and consensus remain in Java
- **Health Checks**: All Rust services provide health status via gRPC
- **Protobuf**: Shared protocol definitions for seamless Java-Rust communication

### HTTP/JSON-RPC APIs
- **Full Node API**: Port 8090 (HTTP), 50051 (gRPC)
- **Solidity Node API**: Separate endpoints for solidified data
- **JSON-RPC**: Compatible interface for web3 clients

### Event System
- **Event Triggers**: Smart contract and transaction events
- **Event Server**: Separate service for event processing
- **Plugins**: Event plugin system for custom processing
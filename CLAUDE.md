# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

+ During you interaction with the user, if you find anything reusable in this project (e.g. version of a library, model name), especially about a fix to a mistake you made or a correction you received, you should take note in the `Lessons` section in the `CLAUDE.md` file so you will not make the same mistake again. 

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

## Lessons learnt

- **gRPC Parameter Validation**: The RemoteStorageSPI constructor fails with NullPointerException when host parameter is null. System.getProperty() can return null, so we need defensive validation in constructors. Always validate critical parameters before using them in external library calls.
- **Protobuf Class References**: Generated protobuf classes are nested within the main `Storage` class (e.g., `storage.Storage.GetRequest`), not standalone classes. Need to use fully qualified names to avoid conflicts with our own classes.
- **Import Strategy**: Use wildcard import `storage.Storage.*` to access all nested protobuf classes without conflicts.
- **Type Conversion**: Java `StorageConfig` uses `Map<String, Object>` but protobuf expects `Map<String, String>`. Convert values using `String.valueOf()`.
- **Error Handling**: Wrap gRPC `StatusRuntimeException` in `RuntimeException` with descriptive messages for proper error propagation.
- **Health Check**: gRPC services need gRPC-based health checks, not HTTP. The performance testing script was failing because it was trying to use HTTP health check on a gRPC service.
- **Gradle Dependency Verification**: When Gradle dependency verification fails, it's usually due to stale verification metadata that needs to be updated for new or changed dependencies.
- **Performance Metrics Collection**: System.out.println() metrics get buried in Gradle test output. Need dedicated metrics files or better output parsing to extract actual performance data.
- **Gradle Test Output**: Gradle suppresses test console output by default. Metrics need to be written to dedicated files (JSON/CSV) rather than relying on console output extraction.
- **Performance Analysis**: Current ~1.17ms PUT / 0.76ms GET latency represents ~20x overhead vs embedded storage (0.054ms PUT / 0.045ms GET), but architectural benefits (crash isolation, operational flexibility) justify the trade-off with planned optimizations.
- **RocksDB Embedded Implementation**: Use `RocksdbStorage` class for embedded benchmarks, need to identify constructor parameters and configuration.
- **JUnit @Test Inheritance**: Methods in abstract base classes without @Test annotations are not recognized as tests by Gradle, even if inherited. Subclasses must override and add @Test annotations to inherited test methods.
- **Checkstyle Warnings**: The performance testing script runs Gradle build which triggers checkstyle checks. Use `-x checkstyleMain -x checkstyleTest` flags to skip checkstyle tasks (not `-x checkstyle` which is ambiguous).
- **Main Application Integration**: The dual storage mode factory pattern was implemented but not integrated into the main java-tron application. The actual FullNode application still uses hardcoded storage initialization in TronDatabase.java and TronStoreWithRevoking.java constructors.
- When compile, always add `--dependency-verification=off` to avoid dependency check
- **Dual Storage Mode**: The project now supports both EMBEDDED (local RocksDB) and REMOTE (gRPC Rust service) storage modes. Configuration is via STORAGE_MODE env var, system property, or config file.
- **Remote Storage Service**: The rust-storage-service must be started separately when using REMOTE mode. It runs on port 50011 by default.
- **gRPC LoadBalancerProvider**: gRPC clients must register the PickFirstLoadBalancerProvider in a static block to avoid "Could not find policy 'pick_first'" errors. The existing DatabaseGrpcClient shows the correct pattern.
- **Test Isolation**: When tests fail due to environment variables or system properties, ensure proper cleanup in @After methods and check for interference between tests.
- Java-tron FullNode should be run with specific JVM parameters: `-Xms9G -Xmx9G -XX:ReservedCodeCacheSize=256m -XX:MetaspaceSize=256m -XX:MaxMetaspaceSize=512m -XX:MaxDirectMemorySize=1G -XX:+PrintGCDetails -XX:+PrintGCDateStamps  -Xloggc:gc.log -XX:+UseConcMarkSweepGC -XX:NewRatio=2 -XX:+CMSScavengeBeforeRemark -XX:+ParallelRefProcEnabled -XX:+HeapDumpOnOutOfMemoryError -XX:+UseCMSInitiatingOccupancyOnly  -XX:CMSInitiatingOccupancyFraction=70` for optimal performance.
- The rust-storage-service process is named `storage-service` when running.
- **Unified Backend Protobuf Generation**: When building multi-crate Rust workspaces with protobuf, avoid duplicate generation by having only one crate generate the proto code and others import it. Duplicate generation causes trait implementation conflicts.
- **Rust Lifetime Management**: When implementing trait methods that return references, use explicit lifetime annotations or return owned types (like `&Box<dyn Trait>`) to avoid complex lifetime issues.
- **Module Configuration**: Rust serde requires all fields in structs to be present in config files. Use empty objects `{}` for optional settings fields in TOML configuration.
- **REVM 14.0 API Changes**: REVM 14.0 has significant API changes from earlier versions. Key fixes: `Output::Create(data, _)` where `data` is `Bytes` not `Option<Bytes>`, environment access through `evm.context.evm.inner.env`, and precompile API changes to struct-based implementations.
- **Rust Module Function Signatures**: When implementing Module trait, `ExecutionModule::new()` takes config by value and returns `Self` (not `Result`), while `StorageModule::new()` takes config by reference and returns `Result<Self>`. Always check function signatures before calling.
- **Database Name Resolution Issue**: RemoteExecutionSPI hardcoded database name as "default" but Rust backend manages multiple named databases (account, block, contract-state, code, etc.). Solution: Remove database parameter from execution operations and let Rust backend route data to appropriate databases automatically based on data type. This maintains java-tron's database separation while providing unified execution interface.
- Protobuf needs extension for full Java-side visibility
- Java code must handle oneof unions properly when protobuf structure changes
- When deserializing data from remote services, always handle variable-length data gracefully
- Account creation in state sync must use the balance from the deserialized data, not default to zero
- Comprehensive logging is essential for debugging state synchronization issues between different systems
- The flow from Rust → Protobuf → Java requires careful attention to serialization formats at each step
- **Manual Protobuf Parsing in Rust**: For simple protobuf messages with few fields, manual wire-format parsing using varint decoding is viable and avoids additional dependencies. Implement `read_varint()` helper and parse fields by tag number. Wire types: 0=varint, 2=length-delimited. This approach works well for Tron system contracts like FreezeBalanceContract.
- **System Contract Implementation Pattern**: When adding new system contract handlers in Rust: (1) Add enum variant to TronContractType, (2) Add config flag to RemoteExecutionConfig with default=false, (3) Add match arm in execute_non_vm_contract with config gate, (4) Implement handler with proper validation/logging/state changes, (5) Emit single AccountChange for CSV parity. Follow existing AccountUpdateContract pattern.
- **Test Infrastructure Limitations**: The Rust test infrastructure has pre-existing issues with mock storage creation (StorageEngine::new_mock doesn't exist). Tests should focus on logic correctness; integration tests require actual storage setup. Unit test compilation errors in existing code don't block library compilation.
- **Config Documentation**: Always document new config flags in both the struct definition (common/src/config.rs) and the config.toml file with clear comments about purpose, defaults, and rollout implications.
- **Witness Storage Format Compatibility (Java-Rust)**: Java stores witnesses as `protocol.Witness` protobuf bytes, while Rust initially used a custom binary format. Solution: Implement dual-decoder in Rust that tries protobuf first (for Java compatibility), then falls back to legacy custom format. For writes, use protobuf by default via `serialize_protobuf()` to unify with Java. Address conversion: Java uses 21-byte TRON format (0x41 prefix + 20 bytes), Rust uses 20-byte; handle both by stripping/adding 0x41 prefix. Validate `voteCount` i64→u64 conversion and reject negatives. This pattern applies to any Java-Rust storage format mismatches.
- **Resource Ledger Storage Pattern**: For freeze/unfreeze ledgers, use compact binary serialization: 8-byte amount (big-endian) + 8-byte expiration timestamp (big-endian) = 16 bytes total. Storage key format: `{0x41}{20-byte address}{resource_type_u8}` (22 bytes). This matches Tron's address prefix convention and allows per-resource tracking. Use `checked_add()` for overflow protection when aggregating amounts.
- **Timestamp Handling**: Java-tron uses milliseconds since epoch for timestamps (i64). When calculating expiration: `expiration = (block_timestamp_u64 + duration_days * 86400 * 1000) as i64`. Always verify timestamp type mismatches between u64 and i64.
- **CSV Parity with Ledger Updates**: When adding resource ledger persistence, maintain CSV parity by NOT emitting StorageChanges for freeze records by default. Use a config flag (`emit_freeze_ledger_changes: false`) to gate ledger emissions. This allows Phase 2 implementation without breaking Phase 1 CSV compatibility.

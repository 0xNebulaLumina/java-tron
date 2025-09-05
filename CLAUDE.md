# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

+ During you interaction with the user, if you find anything reusable in this project (e.g. version of a library, model name), especially about a fix to a mistake you made or a correction you received, you should take note in the `Lessons` section in the `CLAUDE.md` file so you will not make the same mistake again. 
+ You should also use the `CLAUDE.md` file as a scratchpad to organize your thoughts. Especially when you receive a new task, you should first review the content of the scratchpad, clear old different task but keep lessons learned, then explain the task, and plan the steps you need to take to complete the task. You can use todo markers to indicate the progress, e.g.
[X] Task 1
[ ] Task 2

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

## Plan: Parity for Bandwidth Semantics and CSV Encoding

Context: We observed mismatches between (embedded execution + embedded storage) and (remote execution + remote storage) runs starting from the first TransferContract. The deltas point to Java applying bandwidth/TRX adjustments locally that the Rust backend did not see. We will: (1) move TRON bandwidth/TRX fee semantics into the Rust backend so it emits authoritative state changes; (2) normalize the embedded CSV account-change encoding to match the remote format for digest parity.

### Goals
- Rust becomes the source of truth for non-VM TRX transfer resource/fee accounting and emits complete state deltas.
- Embedded CSV export serializes account changes in the same layout as RemoteExecutionSPI to align digests.
- Preserve existing RocksDB storage layouts in Java (only CSV/digest serialization changes on the embedded path).

### Phase 1 — Rust: Implement TRON Bandwidth/TRX Fee Semantics

Owner: rust-backend

Scope: Non-VM value transfers (no data, to EOA). VM paths remain unchanged for now.

Key Design Points
- Read dynamic properties and resource usage from RocksDB via StorageEngine (no hardcoded constants).
- Apply consumption order: free bandwidth → staked/delegated net → TRX fee (burn or blackhole), matching Java semantics.
- Return all resulting state changes (sender account, recipient account, resource usage updates, optional blackhole credit) in `ExecutionResult` for Java to persist.
- Deterministic state change ordering (address asc; account changes before storage for same addr).

Detailed TODOs
[ ] Config: Introduce flag `execution.fees.use_dynamic_properties=true` to enable Rust-side fee semantics (default off until rollout).
[ ] Resource store: Implement readers/writers for resource-related keys (freeNet usage, latest op time, staked net, delegations) mirroring Java DB namespaces (e.g., `properties`, `DelegatedResource`, `DelegatedResourceAccountIndex`).
[ ] Calculator: Compute `bandwidth_used` from tx payload; determine available free bandwidth (windowed), staked/delegated bandwidth, remainder requiring TRX fee using `bandwidth_price` from dynamic properties.
[ ] Applier: Produce state deltas:
    - Sender: balance -= (value + fee), update `latest_op_time`, update usage records.
    - Recipient: balance += value, create if needed.
    - Fee mode = burn: no account delta; mode = blackhole: credit blackhole account (create if needed).
[ ] Core service: Replace current non-VM path post-processing with the new resource manager when the flag is enabled; ensure the gRPC `ExecutionResult` carries all state changes already sorted.
[ ] Logging/metrics: Emit structured debug for `bandwidth_used`, `free_applied`, `staked_applied`, `fee_applied`, `fee_mode`, `blackhole_credit`. Add counters: `resource.free.bytes`, `resource.staked.bytes`, `resource.fee.sun`.
[ ] Edge cases: Window reset on expiry; insufficient funds (value + fee); invalid blackhole address fallback to burn; idempotency guarantee (execution is stateless; Java persists once).
[ ] Tests (unit): free-only, staked-only, fee-required, window rollover, blackhole credit creation.
[ ] Tests (integration, mock engine): seed minimal props/accounts; validate emitted state deltas for representative scenarios.

Out of Scope (Phase 1)
- VM fee semantics (remain disabled: `experimental_vm_blackhole_credit=false`).
- Changing Java storage layouts.

### Phase 2 — Java: Normalize Account-Change Encoding for CSV/Digests

Owner: framework (Java)

Scope: CSV/digest export only. Storage encoding stays as-is.

Target Encoding (to match RemoteExecutionSPI)
- Account value bytes in state change: `[balance(32)][nonce(8)][codeHash(32)][codeLen(4)][code]`.
- For EOAs: `nonce=0`, `codeHash=keccak256("") = c5d246...`, `codeLen=0`, `code=empty`.
- Account change key remains empty (`key_len=0`) to indicate account-level mutation.

Detailed TODOs
[ ] Add a `csv.normalizedAccountEncoding=true` config toggle (default ON for parity workflows).
[ ] In the CSV/logger path (ExecutionCsvLogger and friends), detect account-level state changes and serialize values with the normalized layout above instead of legacy `[balance][latestOpTime][flag]` layout when the flag is set.
[ ] Ensure deterministic ordering of state changes in CSV mirrors Rust’s comparator (address asc; account changes before storage for same address).
[ ] Keep legacy path available behind the toggle for A/B debugging.
[ ] Tests: Golden CSV rows for a few account-change cases (EOA transfer, contract account with code), verifying `state_changes_json` formatting and `state_digest_sha256` stability.

### Phase 3 — Rollout, Validation, and Observability

Rollout Plan
[ ] Dev: Enable `csv.normalizedAccountEncoding=true`; keep Rust fee flag OFF. Verify CSV parity shape.
[ ] Staging: Enable Rust fee flag `execution.fees.use_dynamic_properties=true`. Validate deltas and CSV parity on sampled ranges (e.g., blocks 300–1800).
[ ] Production-like: Gradual enablement; monitor metrics and mismatch alerts.

Cross-Run Validation
[ ] Compare embedded-embedded vs remote-remote for early blocks; confirm the first previously mismatched tx now matches balances and `state_digest_sha256`.
[ ] Spot-check contract account changes (if any) to ensure code hash/length are consistent across both.

Observability & Safeguards
[ ] Java safeguard: before applying remote deltas, if local-old differs from remote-old beyond 0 (expected to be equal post-migration), log a warning with address and both values for investigation.
[ ] Kill switch: both flags (`use_dynamic_properties`, `csv.normalizedAccountEncoding`) must be revertible at runtime/config restart.

### Phase 4 — Parity Hardening (Post-MVP)
[ ] Extend parity to VM paths when ready; move VM fee semantics into Rust with the same authoritative pattern.
[ ] Digest canonicalization (optional): define a semantic hash over fields (balance/nonce/codeHash/codeLen/code) to future-proof against representation drift.
[ ] Broaden test coverage to include delegated resource edge cases and blackhole optimization interactions.

### Acceptance Criteria
- The first mismatched transaction (block 342) shows identical sender/recipient balances across runs and identical `state_changes_json`/`state_digest_sha256`.
- Subsequent sampled transactions maintain parity; any residual mismatches are investigated via the logging/metrics added above.

### Notes & Assumptions
- Java remains the committer of state to local RocksDB; Rust returns authoritative deltas. No double application of fees post migration.
- Dynamic properties and resource usage locations must match Java’s DB schema; we will mirror the key strategy to avoid schema drift.

### Task Tracking (High-Level)
[ ] Phase 1: Rust resource manager (config, store, calculator, applier, service integration, tests)
[ ] Phase 2: Java CSV normalization (toggle, serializer, ordering, tests)
[ ] Phase 3: Rollout + validation (env toggles, comparisons, metrics)
[ ] Phase 4: Hardening (VM parity, canonical digests, extended tests)

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


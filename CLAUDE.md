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

## Current Task: TRON‑Accurate Fee Handling (Remote Execution)

Context
- We compared embedded (execution+storage) vs remote (execution+storage) CSVs and observed systematic mismatches in `state_change_count` and state digest for many transactions. Remote execution appears to emit an EVM-style coinbase credit (miner tip) that should not exist on TRON.
- TRON fee semantics: no per‑tx miner/coinbase payout. Non‑VM txs pay flat bandwidth fees (burn or credit blackhole depending on `supportBlackHoleOptimization`). Witness rewards occur at block finalization, not per tx. VM txs consume energy and still do not credit coinbase.

Objective
- Modify the Rust backend execution path so it never emits Ethereum coinbase payouts and handles non‑VM fees accurately (burn vs. blackhole credit), bringing CSV parity with embedded: correct `state_change_count`, `energy_used` (0 for non‑VM), and matching state digests.

Non‑Goals (for this iteration)
- Implement full TRON fee accounting (stake/energy/bandwidth deduction, fee pool dynamics) identical to Java actuators.
- Change Java caller behavior unless gated behind explicit feature flags in a later phase.

Acceptance Criteria
- No `AccountChange` attributed to block coinbase/miner in remote results for any tx.
- Non‑VM value transfers: `energy_used = 0`; only two account deltas (sender minus amount+fee, recipient plus amount) plus optional blackhole credit if configured. If burn mode is on, no third-party credit delta is emitted.
- Execution CSV compare shows near‑100% accuracy for `state_change_count` and `state_digest_sha256` on the same tx set; `energy_used` aligns (0 for non‑VM).

High‑Level Plan (Phased)
1) Phase 1 – Parity Fix (no proto change):
   - Suppress EVM coinbase/priority fee at the source and stop enforcing Ethereum gas minima.
   - Post‑process to stabilize state change ordering for digest parity.
   - Simple non‑VM heuristic for 0 energy without adding fee deltas.
2) Phase 2 – Configurable TRON Fee Policy (no proto change):
   - Introduce `execution.fees` config (burn vs blackhole) and optional blackhole credit emission for VM path (default off) and non‑VM (conservative).
3) Phase 3 – Full Non‑VM Handling (proto + Java update):
   - Add tx kind to proto; process non‑VM fully in Rust without EVM; apply accurate fee semantics including blackhole credit or burn based on dynamic properties/config.

Key Code Touchpoints
- `rust-backend/crates/core/src/service.rs`
  - `convert_protobuf_transaction(...)`
  - `convert_protobuf_context(...)`
  - `convert_execution_result_to_protobuf(...)`
- `rust-backend/crates/execution/src/tron_evm.rs`
  - `setup_environment(...)`
  - `execute_transaction_with_state_tracking(...)`
  - `extract_state_changes_from_db(...)`
- `rust-backend/crates/execution/src/storage_adapter.rs`
  - Address utils (promote Base58 Tron → EVM address decoder from test to prod)
- `rust-backend/crates/common/src/config.rs` and `rust-backend/config.toml`
  - Add `execution.fees.*` configuration

Detailed TODOs

Phase 1 — Parity Fix (no proto changes)
[ ] Suppress coinbase/priority fee credit
- [ ] In `service.rs:convert_protobuf_transaction`, force `gas_price = 0` regardless of input, with a safety gate `execution.evm_eth_coinbase_compat` (default false). Document that this is for TRON parity.
- [ ] In `tron_evm.rs:setup_environment`, set `env.block.basefee = 0` explicitly (if field exists in current REVM version). Keep `block.coinbase` set for opcode COINBASE correctness but ensure no rewards are distributed.

[ ] Remove Ethereum‑specific gas minima
- [ ] In `tron_evm.rs:execute_transaction_with_state_tracking`, remove the `tx.gas_limit < 21000` rejection. Only enforce `tx.gas_limit <= context.block_gas_limit`. Log a warning if the gas is unusually low to aid debug.

[ ] Deterministic state change ordering (digest parity)
- [ ] After `extract_state_changes_from_db()` returns, sort `state_changes` deterministically before returning the result:
  - AccountChange: by `address` ascending.
  - StorageChange: by `(address, key)` ascending.
- [ ] Keep sorting local to execution result (do not mutate storage records order).

[ ] Non‑VM heuristic energy fix (safe and conservative)
- [ ] Define “likely non‑VM” as `tx.data.is_empty()` AND `to` present AND `code(to) is None`.
- [ ] If likely non‑VM, set `energy_used = 0` in the final `TronExecutionResult`. Do not add any fee deltas here; leave fee effects to Java for now (this avoids accidental double‑counting).
- [ ] Add debug logging when this fast‑path triggers (include `from`, `to`, amount, and reason).

[ ] Unit tests (minimal)
- [ ] Ensure no `AccountChange` for `block_coinbase` even when `energy_used > 0`.
- [ ] Ensure sorting: two identical runs produce identical `state_changes` order.
- [ ] Ensure non‑VM heuristic sets `energy_used = 0` when `to` has no code and `data` is empty.

[ ] Validation
- [ ] Re‑run `scripts/execution_csv_compare.py` on the same tx windows; aim for ~100% on `state_change_count` and state digest.
- [ ] Manually spot‑check transactions previously showing a third account delta (coinbase) — confirm absence.

Phase 2 — Configurable Fee Policy (no proto change)
[ ] Configuration and plumbing
- [ ] Extend `ExecutionConfig` with nested `ExecutionFeeConfig`:
  - `mode: "burn" | "blackhole" | "none"` (default: `"burn"`).
  - `support_black_hole_optimization: bool` (default: true).
  - `blackhole_address_base58: String` (default empty; required if `mode=blackhole`).
  - `experimental_vm_blackhole_credit: bool` (default: false; disabled by default to avoid double‑counting).
  - `non_vm_blackhole_credit_flat: Option<u64>` (SUN), optional flat fee for non‑VM when not deriving from dynamic props.
- [ ] Add TOML examples under `[execution.fees]` and env overrides, e.g. `TRON_BACKEND__EXECUTION__FEES__MODE`.

[ ] Address utilities
- [ ] Promote `from_tron_address(...)` from `#[cfg(test)]` to production (new `common::addr` module or public in `storage_adapter.rs`).
- [ ] Validate checksum and 0x41 prefix; unit test round‑trip with known addresses.

[ ] Optional blackhole credit emission (careful defaults)
- [ ] After extracting and sorting state changes, if `fees.mode = "blackhole"` AND `experimental_vm_blackhole_credit = true`, append a synthetic `AccountChange` crediting blackhole by `estimated_fee = energy_used * context.energy_price` (approximation). Default OFF.
- [ ] For likely non‑VM (heuristic), if `fees.mode = "blackhole"` AND `non_vm_blackhole_credit_flat` is set, append a synthetic `AccountChange` to blackhole for that flat value. Default NONE.
- [ ] Do NOT emit anything in burn mode (no state deltas for fee sinks).
- [ ] Add guard logs indicating this is an approximation until Phase 3.

[ ] Tests and validation
- [ ] Unit test: blackhole credit emission only when enabled; amount matches calculation; address decoding works.
- [ ] CSV compare again: ensure no regressions to `state_change_count` parity in default config (`mode=burn`).

Phase 3 — Full Non‑VM Handling (proto + Java update)
[ ] Protobuf
- [ ] Add `enum TxKind { NON_VM = 0; VM = 1; }` and `tx_kind` in `TronTransaction`.
- [ ] Regenerate and update Java caller to populate `tx_kind`.

[ ] Execution path
- [ ] In core service, branch on `tx_kind`:
  - For `NON_VM`: bypass EVM entirely. Use `StorageModuleAdapter` to load sender/recipient and apply TRON value transfer and fee semantics.
  - `energy_used = 0`; compute `bandwidth_used` based on payload size per TRON rules; update `resource_usage` if needed.
  - Fee handling:
    - If `fees.mode="burn"`: no state delta (supply accounting is elsewhere).
    - If `fees.mode="blackhole"`: credit blackhole account by the fee.
- [ ] For `VM`: continue REVM execution; still no per‑tx miner/coinbase credit.

[ ] Dynamic properties integration (optional)
- [ ] Read `supportBlackHoleOptimization` and fee parameters from dynamic properties DB (via `StorageModuleAdapter`) to auto‑select fee mode and amounts; config acts as fallback.

[ ] Tests and validation
- [ ] Unit tests for non‑VM path: balance debits/credits, burn/no delta vs blackhole credit.
- [ ] End‑to‑end CSV compare in both modes (burn and blackhole) across a block window with mixed tx types.

Risk Mitigation & Compatibility
- Default behavior remains parity‑safe: coinbase suppressed, `fees.mode = burn`, experimental emissions OFF.
- Introduce a temporary `execution.evm_eth_coinbase_compat` flag (default false) to restore old behavior if needed during rollout.
- Sorting only affects return payload ordering, not persisted DB order.

Open Questions / Follow‑ups
- What exact fee values should be emitted for non‑VM remote path to match Java actuators? If dynamic properties are required, Phase 3 should include reading them to compute accurate fees.
- Should remote execution ever emit fee‑related deltas for VM txs, or should all fee effects remain Java‑side until full parity is proven? Current proposal keeps VM fees non‑emitting by default.
- If state digest mismatch persists after coinbase suppression and sorting, audit REVM vs Java EVM differences (e.g., refunds, precompile side‑effects, account creation edge cases) on the mismatching tx set.

Owner Map (by file)
- `crates/core/src/service.rs`: tx/context conversion, result conversion, optional non‑VM heuristic and fee post‑processing gates.
- `crates/execution/src/tron_evm.rs`: env setup, gas/basefee handling, state change extraction and sorting, removal of Ethereum gas minima.
- `crates/execution/src/storage_adapter.rs`: address utilities (Base58 decode), optional account/code queries for heuristics.
- `crates/common/src/config.rs` + `rust-backend/config.toml`: config struct and defaults for `execution.fees.*`, rollout flags.

Verification Checklist (before merge)
[ ] Unit tests added/updated for coinbase suppression, sorting, heuristics, address utils.
[ ] Default config produces no coinbase deltas; CSV compare shows improved parity on provided sample files.
[ ] Docs: `config.toml` and README updated with `execution.fees` and rollout flags.
[ ] Logging at debug level for new branches; no excessive info-level noise.
[ ] Backout plan documented (`execution.evm_eth_coinbase_compat=true`).

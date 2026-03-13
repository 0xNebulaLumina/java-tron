# TODO / Fix Plan: `TRANSFER_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity risks identified in `planning/review_again/TRANSFER_CONTRACT.planning.md`.

## 0) Decide the "parity target" (do this first)

- [x] Confirm desired scope:
  - [x] **Actuator-only parity** (match `TransferActuator.validate + execute`) ← chosen
  - [ ] **End-to-end parity** (also match BandwidthProcessor/AEXT outcomes in remote mode) — deferred
- [x] Confirm network expectations:
  - [x] enforce strict `DecodeUtil.addressValid` semantics for both owner/to (21 bytes, prefix == configured prefix)
  - [x] decide whether the Rust side must accept fixtures with malformed addresses and still match Java's *error ordering* — yes, Rust now accepts malformed `to` without failing early
- [x] Confirm which feature flags/dynamic properties are assumed in the target environment:
  - [x] `ALLOW_MULTI_SIGN` — already handled
  - [x] `supportBlackHoleOptimization` — already handled
  - [x] `FORBID_TRANSFER_TO_CONTRACT` — already handled
  - [x] `ALLOW_TVM_COMPATIBLE_EVM` — already handled

## 1) Fix `toAddress` validation to match `DecodeUtil.addressValid`

Goal: match Java validation for `toAddress`:

- non-empty
- exactly 21 bytes
- prefix byte == `storage_adapter.address_prefix()`
- error message: `"Invalid toAddress!"`

Checklist:

- [x] Extend `tron_backend_execution::TxMetadata` (`rust-backend/crates/execution/src/tron_evm.rs`) to carry `to_raw: Option<Vec<u8>>` (analogous to `from_raw`). — was already present
- [x] Populate `to_raw` in gRPC conversion (`rust-backend/crates/core/src/service/grpc/conversion.rs`) from `tx.to`. — was already present
- [x] In `execute_transfer_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [x] Validate `to_raw` using the same rules as Java (21 bytes + prefix match).
  - [x] Only after validation, derive the 20-byte EVM `to` address (strip prefix) for storage access and comparisons.
  - [x] Keep error ordering: owner-address validation must happen before to-address validation.

## 2) Avoid early conversion failure for malformed `to` addresses (error-order parity)

Goal: allow contract-level validation to produce Java's error messages (and ordering) instead of failing during protobuf→Address conversion.

- [x] Update `convert_protobuf_transaction()` (`rust-backend/crates/core/src/service/grpc/conversion.rs`):
  - [x] Add an "allow malformed `to`" path for NON_VM system contracts that validate raw `to` bytes themselves (at minimum `TransferContract`; possibly others).
  - [x] If `strip_tron_address_prefix(tx.to)` fails, do not return an error immediately:
    - [x] store `to_raw` anyway
    - [x] set `transaction.to` to `None` (lets executor validate `to_raw` and return `"Invalid toAddress!"` consistently)
- [x] Add/extend conformance tests or fixtures for:
  - [x] invalid owner + invalid to (must return `"Invalid ownerAddress!"`) — test: `test_transfer_invalid_owner_and_invalid_to_returns_owner_error_first`
  - [x] valid owner + invalid to (must return `"Invalid toAddress!"`) — test: `test_transfer_valid_owner_and_invalid_to_returns_to_error`
  - [x] wrong-prefix to (must return `"Invalid toAddress!"`) — test: `test_transfer_wrong_prefix_to_address_rejected`

## 3) Clarify/remove the extra flat fee (`fee_amount`) if strict actuator parity is required

Goal: avoid Rust-only fee semantics that don't exist in `TransferActuator`.

- [x] **Option A (strict parity)**: remove `fee_config.non_vm_blackhole_credit_flat` from `TRANSFER_CONTRACT` execution (keep create-account-fee only).
  - [x] Set `fee_amount = 0` unconditionally (matches Java's `TRANSFER_FEE = 0`)
  - [x] Removed the entire `fee_amount > 0` handling block (dead code)
  - [x] Simplified fee calculation: `fee_i64 = create_account_fee` only
  - [x] Added `debug_assert_eq!` guard for parity enforcement
- [ ] ~~**Option B (keep feature, but make it Java-compatible)**~~ — not chosen

## 4) Bandwidth/AEXT parity work (end-to-end parity)

Goal: align Rust's `bandwidth_used`/AEXT side effects with Java's `BandwidthProcessor`.

- [x] Replace `calculate_bandwidth_usage(...)` with a Java-equivalent size computation:
  - [x] Java computes `bytesSize` in `RemoteExecutionSPI.buildExecuteTransactionRequest()` using `clearRet().getSerializedSize() + contracts * MAX_RESULT_SIZE_IN_TX` and passes it via `ExecuteTransactionRequest.transaction_bytes_size`.
  - [x] Rust `calculate_bandwidth_usage()` returns the Java-computed value when present, falls back to hardcoded approximation for backward compatibility.
- [x] Fix `ResourceTracker::increase()` to match Java's precision-scaled algorithm:
  - [x] Use `divideCeil()` for usage normalization
  - [x] Use `f64` decay with `.round()` for `Math.round()` parity
  - [x] Use `PRECISION` constant (1_000_000) and `DEFAULT_WINDOW_SIZE` (28800)
  - [x] Returns `total * windowSize / PRECISION` matching Java's `getUsage()`
- [x] Fix `headSlot` computation to use genesis offset:
  - [x] Added `genesis_block_timestamp` config (default: 1529891469000 mainnet)
  - [x] Compute `now_slot = (block_timestamp - genesis_block_timestamp) / 3000`
- [x] Implement CREATE_ACCOUNT bandwidth path for `TransferContract` that creates a recipient:
  - [x] Added `BandwidthPath::CreateAccount` variant
  - [x] Use `CREATE_NEW_ACCOUNT_BANDWIDTH_RATE` ratio (`netCost = bytesSize * ratio`)
  - [x] Apply windowed usage updates using `now = headSlot`
- [x] Implement ACCOUNT_NET logic (net limit derived from freezes and `TOTAL_NET_WEIGHT/TOTAL_NET_LIMIT`):
  - [x] Read frozen bandwidth via `get_freeze_record(owner, 0)`
  - [x] Calculate `account_net_limit = (frozen / TRX_PRECISION) * (totalNetLimit / totalNetWeight)`
- [x] Implement FREE_NET logic with global public net pool updates:
  - [x] Check both account `free_net_limit` AND global `public_net_limit`
  - [x] Update global `PUBLIC_NET_USAGE` and `PUBLIC_NET_TIME` when FREE_NET path used
- [x] Implement FEE path with fee amount tracking:
  - [x] Added `get_transaction_fee()` dynamic property (default 10 SUN/byte)
  - [x] `fee_amount = bytes_used * transaction_fee`
- [x] Added `BandwidthParams`/`BandwidthResult` structs for full-parameter bandwidth tracking
- [x] Kept legacy `track_bandwidth()` as backward-compatible wrapper

## 5) Verification

- [x] Rust:
  - [x] `cd rust-backend && cargo test` — 285 passed, 3 failed (pre-existing VoteWitness failures, unrelated)
  - [x] Add unit tests specifically for TransferContract validation edge cases (invalid to, wrong prefix, etc.) — 12 new tests in `transfer.rs`
  - [x] Add bandwidth path tests — 13 new tests in `transfer.rs::bandwidth_tests` module
    - [x] `increase()` formula parity: no prior, same slot, full expired, partial decay, quarter decay, decay+usage, zero window
    - [x] Path selection: ACCOUNT_NET, FREE_NET, FEE, CREATE_ACCOUNT, global limit blocking
    - [x] headSlot genesis offset computation
  - [x] `./scripts/ci/run_fixture_conformance.sh --rust-only` — all conformance tests passed
- [ ] Java:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.actuator.TransferActuatorTest"` — not needed for this change (Rust-only modifications)
  - [ ] If validating remote parity: run the relevant remote/dual-mode integration tests and compare CSV state digests.

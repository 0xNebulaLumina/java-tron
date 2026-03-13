# SET_ACCOUNT_ID_CONTRACT (19) — Fix plan / TODO checklist

Goal: if you want stricter Rust↔Java parity (and/or robustness against malformed inputs), close the gaps identified in `planning/review_again/SET_ACCOUNT_ID_CONTRACT.planning.md`.

## A) Confirm current behavior with targeted reproductions
- [x] Run/inspect existing java-tron actuator tests: `./gradlew :framework:test --tests "org.tron.core.actuator.SetAccountIdActuatorTest"`
  - N/A: This is a Java-side test; Rust tests are the focus.
- [x] Run/inspect conformance fixtures for SetAccountId (if you use them) and confirm error strings match the java-tron baseline.
  - No existing SetAccountId conformance fixtures. All existing conformance tests pass.
- [x] Add a conformance case where `tx.from` differs from `contract.owner_address` (only possible with a crafted gRPC request, not via `RemoteExecutionSPI`) to prove the divergence is real.
  - Added test `test_set_account_id_uses_contract_owner_address` in `set_account_id.rs` — proves Rust now uses `owner_address` from contract bytes (not `tx.from`).
- [x] Add a conformance case for 20-byte owner bytes in the gRPC request and confirm java-tron rejects while Rust accepts (today).
  - Added test `test_set_account_id_rejects_20_byte_owner_address` — Rust now rejects 20-byte addresses, matching Java.
- [ ] (Optional) Add a locale-focused reproduction:
  - [ ] Run java-tron with `-Duser.language=tr -Duser.country=TR` and an account_id containing `I` to see how `AccountIdIndexStore` lowercases on that JVM.
  - [ ] Compare to Rust's `account_id_key` behavior.
  - *Skipped*: Locale-dependent behavior is a Java-side issue. Rust now uses deterministic ASCII-only lowercasing, which is correct for the valid account ID character set (0x21–0x7E).

## B) Fix 1: Parse and validate `owner_address` from contract bytes
- [x] Extend `parse_set_account_id_contract` (or add a new parser) in `rust-backend/crates/core/src/service/mod.rs` to extract:
  - [x] `account_id` (field 1)
  - [x] `owner_address` (field 2)
- [x] Apply the same validation order/messages as java-tron:
  - [x] Validate `account_id` first (`Invalid accountId`)
  - [x] Validate `owner_address` as a TRON address (`Invalid ownerAddress`)
- [x] Convert `owner_address` (21 bytes) to the internal 20-byte `Address` and use it as the account key (instead of `transaction.from`), or at minimum:
  - [x] Assert it matches `transaction.metadata.from_raw` / `transaction.from` and fail with the same error message java-tron would produce.
  - *Implementation*: Derive `owner` from `owner_address_bytes[1..]` (matching the pattern used by ProposalDeleteContract), not from `transaction.from`.

## C) Fix 2: Tighten owner address validation to match DecodeUtil
- [x] Decide whether strict parity is required:
  - [x] If yes: require 21-byte address with correct prefix (and reject 20-byte `from_raw` for this contract).
  - *Decision*: Yes — strict parity. Require exactly 21 bytes with 0x41 prefix, matching Java's `DecodeUtil.addressValid()`.

## D) Fix 3: Lowercasing behavior (determinism + parity)
Decision point: do you want "match java-tron as written" or "enforce deterministic behavior"?
- [x] If determinism is the goal: implement ASCII-only lowercasing (A–Z) for account IDs (since validAccountId restricts bytes to printable ASCII) and consider changing java-tron to use `toLowerCase(Locale.ROOT)` for the same reason.

Concrete Rust-side tasks if you choose deterministic ASCII lowercasing:
- [x] Update `account_id_key` in `rust-backend/crates/execution/src/storage_adapter/engine.rs` to perform ASCII lowercase (only `b'A'..=b'Z'`) instead of Unicode `to_lowercase()`.
- [x] Add unit tests that lock:
  - [x] `has_account_id("ABC")` == `has_account_id("abc")`
  - [x] `put_account_id_index("ABC")` is retrievable via `"abc"`

## E) Validation / regression coverage
- [x] Rust tests: `cd rust-backend && cargo test -p tron-backend-core -p tron-backend-execution` (or the narrow set covering service + storage adapter).
  - All 19 new SetAccountId tests pass. 273 total workspace tests pass (3 pre-existing vote_witness failures unrelated to this change).
- [ ] Java tests: `./gradlew :framework:test --tests "org.tron.core.actuator.SetAccountIdActuatorTest"`
  - N/A: Java-side tests; no Java code was modified.
- [x] Run a remote-mode conformance pass if you rely on fixtures (ensure `accountid-index` DB bytes align for the tested cases).
  - All conformance fixtures pass (no SetAccountId-specific fixtures exist yet, but all existing fixtures are green).

## Summary of changes

### Files modified:
1. **`rust-backend/crates/core/src/service/mod.rs`**:
   - `parse_set_account_id_contract`: Now returns `(account_id, owner_address)` tuple instead of just `account_id`. Field 2 (`owner_address`) is extracted instead of skipped.
   - `execute_set_account_id_contract`: Uses `owner_address` from contract bytes (not `transaction.from`) for address validation and account lookup. Validates exactly 21 bytes with correct prefix (matching Java's `DecodeUtil.addressValid`). Removed the old `from_raw`-based validation that accepted 20-byte addresses.

2. **`rust-backend/crates/execution/src/storage_adapter/engine.rs`**:
   - `account_id_key`: Changed from Unicode `to_lowercase()` to ASCII-only lowercasing (`b.is_ascii_uppercase()` → `b.to_ascii_lowercase()`). Made `pub` for testability.

### Files added:
3. **`rust-backend/crates/core/src/service/tests/contracts/set_account_id.rs`**: 19 unit tests covering:
   - Happy path execution
   - Owner address from contract bytes (not tx.from)
   - 20-byte address rejection
   - Wrong prefix rejection
   - Empty owner address rejection
   - Validation order (accountId before ownerAddress)
   - Too short/long account IDs
   - Space in account ID
   - Non-existent account
   - Duplicate account ID
   - Already-set account ID
   - Wrong contract type
   - Case-insensitive uniqueness
   - ASCII lowercasing correctness
   - Case-insensitive has_account_id
   - put/get via lowercase
   - Min/max length boundaries

4. **`rust-backend/crates/core/src/service/tests/contracts/mod.rs`**: Registered `set_account_id` test module.

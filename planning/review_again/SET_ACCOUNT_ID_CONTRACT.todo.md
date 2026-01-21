# SET_ACCOUNT_ID_CONTRACT (19) — Fix plan / TODO checklist

Goal: if you want stricter Rust↔Java parity (and/or robustness against malformed inputs), close the gaps identified in `planning/review_again/SET_ACCOUNT_ID_CONTRACT.planning.md`.

## A) Confirm current behavior with targeted reproductions
- [ ] Run/inspect existing java-tron actuator tests: `./gradlew :framework:test --tests "org.tron.core.actuator.SetAccountIdActuatorTest"`
- [ ] Run/inspect conformance fixtures for SetAccountId (if you use them) and confirm error strings match the java-tron baseline.
- [ ] Add a conformance case where `tx.from` differs from `contract.owner_address` (only possible with a crafted gRPC request, not via `RemoteExecutionSPI`) to prove the divergence is real.
- [ ] Add a conformance case for 20-byte owner bytes in the gRPC request and confirm java-tron rejects while Rust accepts (today).
- [ ] (Optional) Add a locale-focused reproduction:
  - [ ] Run java-tron with `-Duser.language=tr -Duser.country=TR` and an account_id containing `I` to see how `AccountIdIndexStore` lowercases on that JVM.
  - [ ] Compare to Rust’s `account_id_key` behavior.

## B) Fix 1: Parse and validate `owner_address` from contract bytes
- [ ] Extend `parse_set_account_id_contract` (or add a new parser) in `rust-backend/crates/core/src/service/mod.rs` to extract:
  - [ ] `account_id` (field 1)
  - [ ] `owner_address` (field 2)
- [ ] Apply the same validation order/messages as java-tron:
  - [ ] Validate `account_id` first (`Invalid accountId`)
  - [ ] Validate `owner_address` as a TRON address (`Invalid ownerAddress`)
- [ ] Convert `owner_address` (21 bytes) to the internal 20-byte `Address` and use it as the account key (instead of `transaction.from`), or at minimum:
  - [ ] Assert it matches `transaction.metadata.from_raw` / `transaction.from` and fail with the same error message java-tron would produce.

## C) Fix 2: Tighten owner address validation to match DecodeUtil
- [ ] Decide whether strict parity is required:
  - [ ] If yes: require 21-byte address with correct prefix (and reject 20-byte `from_raw` for this contract).
  - [ ] If no (because some callers send 20 bytes): document the divergence and keep the looser acceptance.

## D) Fix 3: Lowercasing behavior (determinism + parity)
Decision point: do you want “match java-tron as written” or “enforce deterministic behavior”?
- [ ] If parity-with-java is the only goal: note that java-tron uses default-locale `toLowerCase()`; reproducing that exactly in a separate Rust process is not feasible without receiving the locale as part of the request.
- [ ] If determinism is the goal: implement ASCII-only lowercasing (A–Z) for account IDs (since validAccountId restricts bytes to printable ASCII) and consider changing java-tron to use `toLowerCase(Locale.ROOT)` for the same reason.

Concrete Rust-side tasks if you choose deterministic ASCII lowercasing:
- [ ] Update `account_id_key` in `rust-backend/crates/execution/src/storage_adapter/engine.rs` to perform ASCII lowercase (only `b'A'..=b'Z'`) instead of Unicode `to_lowercase()`.
- [ ] Add unit tests that lock:
  - [ ] `has_account_id("ABC")` == `has_account_id("abc")`
  - [ ] `put_account_id_index("ABC")` is retrievable via `"abc"`

## E) Validation / regression coverage
- [ ] Rust tests: `cd rust-backend && cargo test -p tron-backend-core -p tron-backend-execution` (or the narrow set covering service + storage adapter).
- [ ] Java tests: `./gradlew :framework:test --tests "org.tron.core.actuator.SetAccountIdActuatorTest"`
- [ ] Run a remote-mode conformance pass if you rely on fixtures (ensure `accountid-index` DB bytes align for the tested cases).


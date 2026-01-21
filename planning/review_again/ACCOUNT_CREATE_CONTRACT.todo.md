# TODO / Fix Plan: `ACCOUNT_CREATE_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity gaps identified in `planning/review_again/ACCOUNT_CREATE_CONTRACT.planning.md`.

## 0) Decide “parity target” (do this first)

- [ ] Confirm desired scope:
  - [ ] **Actuator-only parity** (match `CreateAccountActuator` + `AccountCapsule`)
  - [ ] **End-to-end parity** (also match `BandwidthProcessor` create-account resource path + receipts)
- [ ] Confirm target network mode expectations:
  - [ ] mainnet prefix only (`0x41`)
  - [ ] testnet prefix only (`0xa0`)
  - [ ] must enforce prefix strictly per configured network

## 1) Address validation strictness (Rust should match Java)

Goal: mirror `DecodeUtil.addressValid` semantics: **length==21** and **prefix==configured prefix**.

- [ ] Update `parse_account_create_contract()` in `rust-backend/crates/core/src/service/mod.rs`:
  - [ ] Replace hardcoded `0x41 || 0xa0` allowlist with validation against `storage_adapter.address_prefix()` (or pass the expected prefix into the parser).
  - [ ] Ensure error strings remain Java-parity:
    - [ ] invalid owner → `"Invalid ownerAddress"`
    - [ ] invalid target → `"Invalid account address"`
- [ ] Add/extend Rust tests (prefer unit tests near service parsing code):
  - [ ] mainnet configured, prefix `0xa0` → must fail with invalid address
  - [ ] testnet configured, prefix `0x41` → must fail with invalid address
  - [ ] wrong length (20/22) → must fail

## 2) Respect the contract `type` field

Goal: match Java’s `AccountCapsule(AccountCreateContract, ...)` which stores `type`/`typeValue`.

- [ ] Parse field 3 (`type`, varint) in `parse_account_create_contract()` and return it.
- [ ] When writing `target_proto` in `execute_account_create_contract()`:
  - [ ] Set `target_proto.r#type` (enum numeric value)
  - [ ] If the generated proto struct has a distinct `type_value`, ensure it matches the parsed varint (preserve unknown enum values if needed)
- [ ] Add tests:
  - [ ] type `Normal` produces identical DB bytes to current behavior (regression guard)
  - [ ] non-default type produces expected stored numeric value

## 3) Dynamic property presence parity (optional)

Goal: decide whether Rust should:

- strictly error when critical keys are absent (matching Java’s `IllegalArgumentException`), or
- keep current default fallback behavior (safer for partial DBs) but accept that this diverges.

Checklist:

- [ ] Identify which keys are “must exist” for this contract in real deployments:
  - [ ] `CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT`
  - [ ] `LATEST_BLOCK_HEADER_TIMESTAMP`
  - [ ] `ALLOW_MULTI_SIGN` (+ `ACTIVE_DEFAULT_OPERATIONS` when enabled)
  - [ ] `ALLOW_BLACKHOLE_OPTIMIZATION`
- [ ] If choosing strict parity:
  - [ ] Change getters in `rust-backend/crates/execution/src/storage_adapter/engine.rs` to return errors when missing (instead of defaults), at least when running in conformance mode.
  - [ ] Add tests covering missing-key scenarios and expected error propagation.

## 4) End-to-end resource parity (only if required)

Goal: match `BandwidthProcessor` create-account behavior for `AccountCreateContract`:

- create-account net cost: `netCost = bytes * CREATE_NEW_ACCOUNT_BANDWIDTH_RATE`
- fallback to fee: charge `CREATE_ACCOUNT_FEE` when bandwidth insufficient
- update dynamic totals: `TOTAL_CREATE_ACCOUNT_COST` when fee fallback used

Checklist:

- [ ] Implement dynamic property getter(s) in Rust:
  - [ ] `CREATE_NEW_ACCOUNT_BANDWIDTH_RATE`
  - [ ] `CREATE_ACCOUNT_FEE`
  - [ ] `TOTAL_CREATE_ACCOUNT_COST` update helper
- [ ] Decide where the logic lives:
  - [ ] inside `execute_account_create_contract()` (contract-specific)
  - [ ] or in shared bandwidth accounting used by all non-VM txs (preferred long-term)
- [ ] Update AEXT tracking (if `accountinfo_aext_mode == "tracked"`):
  - [ ] track **netCost** (post-multiplier), not raw bytes
  - [ ] ensure “now” matches Java’s notion (slot/headSlot vs blockNumber)
  - [ ] implement account-net (frozen bandwidth) vs free-net vs fee paths (ResourceTracker is currently simplified)
- [ ] Add conformance-style tests:
  - [ ] bandwidth path success (enough net/free net)
  - [ ] fee fallback path (insufficient bandwidth, sufficient TRX for createAccountFee)
  - [ ] insufficient bandwidth + insufficient TRX → must fail with the same error as Java

## 5) Receipt parity (only if required)

Goal: match Java’s receipt status/fee for this contract in remote mode.

- [ ] Decide whether remote path must set `tron_transaction_result` for all system contracts.
- [ ] If yes:
  - [ ] Use `TransactionResultBuilder` to emit serialized `Protocol.Transaction.Result` bytes equivalent to `ret.setStatus(fee, SUCESS)` for AccountCreateContract.
  - [ ] Add tests in Java (or integration) to verify receipt fields are correct under remote execution.

## 6) Verification steps

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] Run any existing conformance runner for `ACCOUNT_CREATE_CONTRACT` fixtures (if available)
- [ ] Java:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest"`
  - [ ] (If fixture-based) regenerate/compare fixtures for AccountCreate cases


# TODO / Fix Plan: `TRANSFER_ASSET_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity risks identified in `planning/review_again/TRANSFER_ASSET_CONTRACT.planning.md`.

## 0) Decide the “parity target” (do this first)

- [ ] Confirm desired scope:
  - [ ] **Actuator-only parity** (match `TransferAssetActuator.validate + execute`)
  - [ ] **End-to-end parity** (also match remote-mode `state_changes` / CSV expectations)
- [ ] Confirm network expectations:
  - [ ] enforce address prefix strictly (`DecodeUtil.addressValid` semantics)
  - [ ] expected prefix source: `storage_adapter.address_prefix()` vs config
- [ ] Confirm which features are assumed enabled in the target environment:
  - [ ] `ALLOW_MULTI_SIGN` (0 vs 1)
  - [ ] `ALLOW_SAME_TOKEN_NAME` (0 vs 1)
  - [ ] `supportBlackHoleOptimization` (0 vs 1)

## 1) Fix recipient account creation permissions (`ALLOW_MULTI_SIGN == 1`)

Goal: when TRC-10 transfer creates a missing recipient, mirror java-tron’s
`new AccountCapsule(..., withDefaultPermission=true, dynamicStore)` behavior.

- [ ] Update `execute_trc10_transfer_contract()` in `rust-backend/crates/core/src/service/mod.rs`:
  - [ ] When `recipient_proto_opt.is_none()`:
    - [ ] Read `ALLOW_MULTI_SIGN` via `storage_adapter.get_allow_multi_sign()`
    - [ ] If enabled:
      - [ ] Read `ACTIVE_DEFAULT_OPERATIONS` via `storage_adapter.get_active_default_operations()`
      - [ ] Populate `owner_permission` (id=0, threshold=1, key=recipient address weight=1)
      - [ ] Populate `active_permission` (id=2, threshold=1, operations=`ACTIVE_DEFAULT_OPERATIONS`, same key)
    - [ ] Keep error-message parity if any getter fails (should likely surface as execution error).
- [ ] Add a conformance fixture for this case:
  - [ ] In `framework/src/test/java/org/tron/core/conformance/TransferFixtureGeneratorTest.java`:
    - [ ] Add `TransferAssetContract` fixture “creates recipient + allowMultiSign=1”
    - [ ] Ensure expected DB bytes include default permission fields.

## 2) Match Java address validation (`DecodeUtil.addressValid`)

Goal: fail like Java when addresses are malformed: **21 bytes** and **prefix == configured prefix**.

- [ ] Owner address:
  - [ ] In `execute_trc10_transfer_contract()`:
    - [ ] Replace the current `(0x41 || 0xa0 || 20-bytes)` accept logic with:
      - [ ] require `from_raw.len() == 21`
      - [ ] require `from_raw[0] == storage_adapter.address_prefix()`
    - [ ] Keep error string: `"Invalid ownerAddress"`.
- [ ] To address:
  - [ ] Carry raw `to` bytes through to execution:
    - [ ] Add `to_raw: Option<Vec<u8>>` to `tron_backend_execution::TxMetadata`
    - [ ] Set it in `rust-backend/crates/core/src/service/grpc/conversion.rs` from `tx.to`
  - [ ] Validate `to_raw` inside `execute_trc10_transfer_contract()`:
    - [ ] if empty → `"Invalid toAddress"`
    - [ ] if present but invalid length/prefix → `"Invalid toAddress"`
- [ ] Conformance tests:
  - [ ] Add fixture for wrong-prefix owner address (expect `"Invalid ownerAddress"`)
  - [ ] Add fixture for wrong-prefix to address (expect `"Invalid toAddress"`)

## 3) Align empty `asset_name` behavior (error parity)

Goal: when `TransferAssetContract.asset_name` is empty, Java fails with `"No asset!"`.

Options (pick one):

- [ ] **Option A (recommended)**: parse `TransferAssetContract` from `metadata.contract_parameter.value`
  - [ ] Use it as the source-of-truth for `asset_name` bytes (and optionally owner/to bytes).
  - [ ] If `asset_name` empty: return `"No asset!"` (not `"asset_id is required..."`).
- [ ] **Option B (protocol mapping)**: keep using `metadata.asset_id`, but:
  - [ ] include empty `assetId` in the Java→Rust request mapping (don’t drop it to “None”)
  - [ ] map empty bytes to `"No asset!"`.

## 4) `state_changes` parity for create-account fee + blackhole/burn (if needed)

Goal: if remote-mode uses `state_changes` for CSV parity, ensure it reflects the same deltas Java would imply:

- owner balance delta for create-account fee
- optional blackhole account delta (when not burning)

Checklist:

- [ ] Capture `old_account` snapshots *before* mutating/persisting the corresponding accounts.
- [ ] Emit real `AccountChange` entries when:
  - [ ] `create_account_fee > 0` (owner balance changes)
  - [ ] blackhole is credited (balance changes)
- [ ] Keep the existing “no-op AccountChange” behavior only when there is truly no balance delta and you only need AEXT passthrough.
- [ ] Ensure deterministic ordering (`state_changes.sort_by_key(address)`).

## 5) Verification

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] If a conformance runner exists for these fixtures, run it for `TRANSFER_ASSET_CONTRACT`
- [ ] Java:
  - [ ] `./gradlew :framework:test --tests \"org.tron.core.conformance.TransferFixtureGeneratorTest\"`
  - [ ] If remote storage/execution integration is under test: run the relevant dual-mode test target(s)


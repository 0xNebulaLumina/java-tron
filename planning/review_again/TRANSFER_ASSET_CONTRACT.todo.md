# TODO / Fix Plan: `TRANSFER_ASSET_CONTRACT` parity gaps

This checklist assumes we want to resolve the parity risks identified in `planning/review_again/TRANSFER_ASSET_CONTRACT.planning.md`.

## 0) Decide the "parity target" (do this first)

- [x] Confirm desired scope:
  - [x] **Actuator-only parity** (match `TransferAssetActuator.validate + execute`)
  - [x] **End-to-end parity** (also match remote-mode `state_changes` / CSV expectations)
- [x] Confirm network expectations:
  - [x] enforce address prefix strictly (`DecodeUtil.addressValid` semantics)
  - [x] expected prefix source: `storage_adapter.address_prefix()` vs config
- [x] Confirm which features are assumed enabled in the target environment:
  - [x] `ALLOW_MULTI_SIGN` (0 vs 1)
  - [x] `ALLOW_SAME_TOKEN_NAME` (0 vs 1)
  - [x] `supportBlackHoleOptimization` (0 vs 1)

## 1) Fix recipient account creation permissions (`ALLOW_MULTI_SIGN == 1`)

Goal: when TRC-10 transfer creates a missing recipient, mirror java-tron's
`new AccountCapsule(..., withDefaultPermission=true, dynamicStore)` behavior.

- [x] Update `execute_trc10_transfer_contract()` in `rust-backend/crates/core/src/service/mod.rs`:
  - [x] When `recipient_proto_opt.is_none()`:
    - [x] Read `ALLOW_MULTI_SIGN` via `storage_adapter.get_allow_multi_sign()`
    - [x] If enabled:
      - [x] Read `ACTIVE_DEFAULT_OPERATIONS` via `storage_adapter.get_active_default_operations()`
      - [x] Populate `owner_permission` (id=0, threshold=1, key=recipient address weight=1)
      - [x] Populate `active_permission` (id=2, threshold=1, operations=`ACTIVE_DEFAULT_OPERATIONS`, same key)
    - [x] Keep error-message parity if any getter fails (should likely surface as execution error).
- [x] Add a conformance fixture for this case:
  - [x] In `framework/src/test/java/org/tron/core/conformance/TransferFixtureGeneratorTest.java`:
    - [x] Add `TransferAssetContract` fixture "creates recipient + allowMultiSign=1"
    - [x] Ensure expected DB bytes include default permission fields.

## 2) Match Java address validation (`DecodeUtil.addressValid`)

Goal: fail like Java when addresses are malformed: **21 bytes** and **prefix == configured prefix**.

- [x] Owner address:
  - [x] In `execute_trc10_transfer_contract()`:
    - [x] Replace the current `(0x41 || 0xa0 || 20-bytes)` accept logic with:
      - [x] require `from_raw.len() == 21`
      - [x] require `from_raw[0] == storage_adapter.address_prefix()`
    - [x] Keep error string: `"Invalid ownerAddress"`.
- [x] To address:
  - [x] Carry raw `to` bytes through to execution:
    - [x] Add `to_raw: Option<Vec<u8>>` to `tron_backend_execution::TxMetadata`
    - [x] Set it in `rust-backend/crates/core/src/service/grpc/conversion.rs` from `tx.to`
  - [x] Validate `to_raw` inside `execute_trc10_transfer_contract()`:
    - [x] if empty → `"Invalid toAddress"`
    - [x] if present but invalid length/prefix → `"Invalid toAddress"`
- [x] Conformance tests:
  - [x] Add fixture for wrong-prefix owner address (expect `"Invalid ownerAddress"`)
  - [x] Add fixture for wrong-prefix to address (expect `"Invalid toAddress"`)

## 3) Align empty `asset_name` behavior (error parity)

Goal: when `TransferAssetContract.asset_name` is empty, Java fails with `"No asset!"`.

Options (pick one):

- [x] **Option A (recommended)**: parse `TransferAssetContract` from `metadata.contract_parameter.value`
  - [x] Use it as the source-of-truth for `asset_name` bytes (and optionally owner/to bytes).
  - [x] If `asset_name` empty: return `"No asset!"` (not `"asset_id is required..."`).
- [ ] ~~**Option B (protocol mapping)**: keep using `metadata.asset_id`, but:~~ (Not chosen)
  - [ ] ~~include empty `assetId` in the Java→Rust request mapping (don't drop it to "None")~~
  - [ ] ~~map empty bytes to `"No asset!"`.~~
- [x] Conformance test:
  - [x] Add fixture for empty asset_name (expect `"No asset!"`)

## 4) `state_changes` parity for create-account fee + blackhole/burn (if needed)

Goal: if remote-mode uses `state_changes` for CSV parity, ensure it reflects the same deltas Java would imply:

- owner balance delta for create-account fee
- optional blackhole account delta (when not burning)

Checklist:

- [x] Capture `old_account` snapshots *before* mutating/persisting the corresponding accounts.
- [x] Emit real `AccountChange` entries when:
  - [x] `create_account_fee > 0` (owner balance changes)
  - [x] blackhole is credited (balance changes)
- [x] Keep the existing "no-op AccountChange" behavior only when there is truly no balance delta and you only need AEXT passthrough.
- [x] Ensure deterministic ordering (`state_changes.sort_by_key(address)`).

## 5) Verification

- [x] Rust:
  - [x] `cd rust-backend && cargo test` (273 passed, 3 pre-existing vote_witness failures)
  - [x] Conformance runner: `./scripts/ci/run_fixture_conformance.sh --rust-only` (All passed)
- [ ] Java:
  - [ ] `./gradlew :framework:test --tests "org.tron.core.conformance.TransferFixtureGeneratorTest"`
  - [ ] If remote storage/execution integration is under test: run the relevant dual-mode test target(s)

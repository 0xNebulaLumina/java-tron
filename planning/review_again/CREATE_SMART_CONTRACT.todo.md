# TODO / Fix Plan: `CREATE_SMART_CONTRACT` parity gaps

This checklist assumes we want to resolve the gaps identified in `planning/review_again/CREATE_SMART_CONTRACT.planning.md`.

## 0) Decide the parity target (do this first)

- [ ] Confirm desired scope:
  - [ ] **VMActuator-only parity** (match `VMActuator.create()` + stores)
  - [ ] **Full VM parity** (also match internal `CREATE` address derivation + energy/resource caps)
- [ ] Confirm which network/fork settings must be supported:
  - [ ] `ALLOW_TVM_COMPATIBLE_EVM` can be `0` or `1`
  - [ ] `ALLOW_TVM_TRANSFER_TRC10` can be `0` or `1`
  - [ ] Energy-limit hard-fork behavior (if applicable) must be modeled or can be assumed “always on”

## 1) Persist `SmartContract.version` like Java

Goal: match `VMActuator.create()` which forces `version = 1` for newly created contracts when `ALLOW_TVM_COMPATIBLE_EVM == 1`.

- [x] In `rust-backend/crates/core/src/service/mod.rs` (`persist_smart_contract_metadata`):
  - [x] Read `ALLOW_TVM_COMPATIBLE_EVM` from dynamic properties (getter already exists on the storage adapter).
  - [x] If enabled, set `smart_contract.version = 1` before `put_smart_contract(...)`.
  - [x] If disabled, ensure `smart_contract.version` is `0` (or leave as-is if the wire value is already `0`).
- [x] Add tests:
  - [x] "create contract persists version=1 when allow_tvm_compatible_evm=1"
    - Test: `test_create_contract_persists_version_1_when_allow_tvm_compatible_evm_enabled`
  - [x] "create contract persists version=0 when allow_tvm_compatible_evm=0"
    - Test: `test_create_contract_persists_version_0_when_allow_tvm_compatible_evm_disabled`
  - [ ] TransferContract validation against the newly created contract rejects with Java message when version==1 (end-to-end parity check).

## 2) Implement TRON legacy `CREATE` address derivation (txid + internal nonce)

Goal: match Java internal `CREATE` address derivation:

- `TransactionUtil.generateContractAddress(rootTxId, nonce)` → `keccak256(txid || nonce_be_u64)[12..]`
- `nonce` is a **global per-root-tx internal-transaction counter** incremented by calls as well.

Checklist:

- [x] Extend `TronExternalContext` to carry:
  - [x] `root_txid: Option<B256>` (implemented as `root_transaction_id`)
  - [x] `internal_tx_nonce: u64` (init to 0 each top-level tx)
- [x] Set `root_txid` during `TronEvm::setup_environment()` from `ExecutionContext.transaction_id`.
- [x] Override address derivation for **every** legacy `CREATE` (not just the first/top-level), using:
  - [x] top-level CreateSmartContract: still uses `txid + owner_address` (WalletUtil scheme)
  - [x] internal CREATE opcode: uses `txid + internal_tx_nonce` (TransactionUtil scheme) via `derive_internal_create_address()`
- [x] Implement `internal_tx_nonce` increments to match Java ordering:
  - [x] Increment on CALL-start events (via Inspector::call hook)
  - [x] For CREATE: derive address using current nonce, then increment (inside `derive_internal_create_address()`)
  - [x] Confirm whether to count precompile calls the same way Java does: **Confirmed - java-tron does NOT count precompile calls** (`Program.callToPrecompiledAddress` does not call `increaseNonce`). Also, the root tx entry call (depth==0) is not counted. Both behaviors now implemented in Inspector::call hook.
- [x] Add conformance tests:
  - [x] Internal nonce tests in `tron_evm.rs`:
    - `internal_nonce_does_not_count_tx_entry_call` - Verifies root call is not counted
    - `internal_nonce_skips_precompile_calls` - Verifies precompile calls don't increment nonce
    - `test_derive_internal_create_address_formula` - Verifies keccak256(txid||nonce) formula
    - `test_derive_internal_create_address_returns_none_without_txid` - Verifies None returned without txid
    - `test_increment_internal_nonce` - Verifies nonce increment and saturation behavior
    - `test_multiple_calls_increment_nonce` - Verifies multiple CALLs increment nonce correctly
    - `test_tron_vs_ethereum_create_address_differs` - Confirms TRON and ETH schemes differ
  - [x] Address derivation tests in `create_smart_contract.rs`:
    - `test_internal_create_address_derivation_formula` - Pure function test
    - `test_internal_create_addresses_sequence_stable` - Multiple creates produce stable sequence
    - `test_no_address_collisions_across_different_txids` - No collisions with different txids
    - `test_top_level_vs_internal_create_address_differs` - Top-level vs internal derivation differs
  - [ ] Constructor that does `CALL` then `CREATE` and asserts the created sub-contract address matches Java fixture (requires end-to-end EVM execution test with CREATE opcode).

## 3) Apply TRC-10 `call_token_value` transfer for CreateSmartContract

Goal: match Java `MUtil.transferToken(...)` behavior for contract creation when `ALLOW_TVM_TRANSFER_TRC10 == 1`.

- [x] Decide the rollback model:
  - [x] Token transfer must be reverted if contract creation reverts/halts.
  - [x] Emit `Trc10Change` only on successful contract creation (Java applies transfer; rollback natural on failure).
- [x] Implement post-execution token transfer emission for CreateSmartContract:
  - [x] Decode CreateSmartContract proto from tx.data (in `extract_create_contract_trc10_transfer`).
  - [x] Use the contract address from EVM result (already computed by EVM).
  - [x] Emit `Trc10Change::AssetTransferred` so Java can apply the token transfer on success.
  - [x] Added helper function `extract_create_contract_trc10_transfer` in `service/mod.rs`
  - [x] Integrated in grpc handler after successful contract creation
- [x] Add tests in `create_smart_contract.rs`:
  - [x] `test_trc10_transfer_emitted_on_successful_contract_creation` - Token transfer emitted on success
  - [x] `test_trc10_transfer_not_emitted_when_call_token_value_zero` - No transfer when value is 0
  - [x] `test_trc10_transfer_not_emitted_when_trc10_disabled` - No transfer when ALLOW_TVM_TRANSFER_TRC10=0
  - [ ] Token transfer rolled back on REVERT/OOG (natural since Trc10Change not emitted on failure) - Requires end-to-end EVM revert test
  - [x] Java-parity error messages for insufficient token balance / missing asset:
    - `test_trc10_validation_rejects_missing_asset` - "No asset !" error
    - `test_trc10_validation_rejects_zero_asset_balance` - "assetBalance must greater than 0." error
    - `test_trc10_validation_rejects_insufficient_asset_balance` - "assetBalance is not sufficient." error

## 4) Match Java energy/resource capping for contract creation

Goal: approximate `VMActuator.getAccountEnergyLimitWithFixRatio(...)` (and fork-specific rules):

- availableEnergy = leftFrozenEnergy + max(balance - callValue, 0) / energyFee
- energyFromFeeLimit = feeLimit / energyFee
- energyLimit = min(availableEnergy, energyFromFeeLimit)

**Current status**: The Java side sends `feeLimit` as `energyLimit` via RemoteExecutionSPI. Full parity requires
either Java to pre-compute the actual energy limit, or Rust to implement the full frozen energy + window computation.

**Recommended approach**: Have Java compute `getAccountEnergyLimitWithFixRatio()` and send the result in the gRPC request.
This keeps the complex resource accounting logic in Java where it already exists.

Checklist:

- [ ] **Option A (preferred)**: Java-side changes to send computed energy limit:
  - [ ] In RemoteExecutionSPI, call `VMActuator.getAccountEnergyLimitWithFixRatio()` or equivalent
  - [ ] Send the computed limit instead of raw `feeLimit` in the ExecuteTransactionRequest
  - [ ] Rust receives the already-capped value and uses it directly
- [ ] **Option B**: Implement "available energy" computation in Rust using:
  - [ ] account balance
  - [ ] frozen energy (Freeze V1/V2 + recovery windows, if required)
  - [ ] dynamic property `ENERGY_FEE`
- [ ] Set the EVM tx.gas_limit to computed `energyLimit` (plus intrinsic adjustment already done in `TronEvm::setup_environment()`).
- [ ] Add tests:
  - [ ] feeLimit high but balance/frozen energy low → Rust execution halts with OOG where Java would
  - [ ] feeLimit low but balance high → feeLimit cap respected

## 5) Tighten validation + error-message parity (optional but recommended)

- [x] Ensure CreateSmartContract fails when owner account does not exist (even when callValue==0), with Java-parity error text.
  - [x] Added check in `validate_create_smart_contract()` that fails with "Validate InternalTransfer error, no OwnerAccount." when owner account doesn't exist
- [x] Confirm address validity expectations for owner/origin for remote mode:
  - [x] Implemented `is_valid_tron_address()` helper mirroring `DecodeUtil.addressValid`:
    - Not empty
    - Length = 21 bytes
    - First byte = 0x41 (mainnet) or 0xa0 (testnet) prefix
  - [x] Added validation for both owner_address and origin_address with "Invalid ownerAddress" / "Invalid originAddress" errors
- [x] Re-evaluate the "disallow precompile address creation" check for TRON parity (either remove or gate behind a compatibility flag).
  - [x] Java's VMActuator doesn't have this check, so we gated it behind `skip_precompile_create_collision_check` config flag
  - [x] Default: `true` (skip the check to match Java behavior)
  - [x] Set to `false` to enable Ethereum-style collision checking
- [x] Add validation tests in `create_smart_contract.rs`:
  - [x] `test_validation_rejects_missing_owner_account` - "Validate InternalTransfer error, no OwnerAccount." error
  - [x] `test_validation_rejects_contract_name_too_long` - Name > 32 bytes rejected
  - [x] `test_validation_rejects_invalid_percent` - percent > 100 rejected

## 6) Verification steps

- [x] Rust:
  - [x] `cargo build --release` - Compiles successfully
  - [x] `cd rust-backend && cargo test` - Unit tests pass
    - `cargo test --package tron-backend-core create_smart_contract` - 17 tests pass
    - `cargo test --package tron-backend-execution internal_nonce` - 7 tests pass
  - [ ] Run any conformance runner cases that include CreateSmartContract + internal CREATE patterns
- [ ] Java:
  - [ ] `./gradlew :framework:test`
  - [ ] Run dual-mode tests if available: `./gradlew :framework:test --tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest"`

## Implementation Summary

### Files Modified:

1. **`rust-backend/crates/core/src/service/mod.rs`**:
   - `persist_smart_contract_metadata()`: Added `version = 1` when `ALLOW_TVM_COMPATIBLE_EVM == 1`
   - Added `extract_create_contract_trc10_transfer()` for TRC-10 transfer emission

2. **`rust-backend/crates/core/src/service/grpc/mod.rs`**:
   - Integrated TRC-10 transfer emission after successful contract creation

3. **`rust-backend/crates/execution/src/tron_evm.rs`**:
   - Extended `TronExternalContext` with `root_transaction_id` and `internal_tx_nonce`
   - Added `derive_internal_create_address()` for TRON's txid+nonce scheme
   - Added `increment_internal_nonce()` helper
   - Updated `Inspector::call` to increment nonce for CALLs (excludes root call at depth==0 and precompile calls)
   - Updated `tron_create_with_optional_override` to use TRON derivation for internal CREATEs
   - Updated `setup_environment` to initialize root txid and nonce
   - Added `skip_precompile_create_collision_check` flag to `TronExternalContext` (set from config)
   - Gated precompile collision check behind the flag for Java parity
   - Added unit tests: `internal_nonce_does_not_count_tx_entry_call`, `internal_nonce_skips_precompile_calls`

4. **`rust-backend/crates/execution/src/lib.rs`** (Section 5 additions):
   - Added `is_valid_tron_address()` helper mirroring Java's `DecodeUtil.addressValid()`
   - Added owner address validity check with "Invalid ownerAddress" error
   - Added origin address validity check with "Invalid originAddress" error
   - Added owner account existence check with "Validate InternalTransfer error, no OwnerAccount." error
   - Reordered validation steps for clearer Java parity

5. **`rust-backend/crates/common/src/config.rs`** (Section 5 additions):
   - Added `skip_precompile_create_collision_check` config flag to `ExecutionConfig`
   - Default: `true` (skip check for Java parity; Java doesn't have this validation)

6. **`rust-backend/crates/core/src/service/tests/contracts/create_smart_contract.rs`** (NEW - 17 Tests):
   - SmartContract.version persistence tests (ALLOW_TVM_COMPATIBLE_EVM)
   - Internal CREATE address derivation formula tests
   - Address sequence stability tests
   - TRC-10 call_token_value transfer tests
   - Validation parity tests (missing owner account, name too long, invalid percent)
   - TRC-10 validation error message parity tests ("No asset !", "assetBalance must greater than 0.", "assetBalance is not sufficient.")

7. **`rust-backend/crates/core/src/service/tests/contracts/mod.rs`**:
   - Added `mod create_smart_contract;` declaration

8. **`rust-backend/crates/execution/src/tron_evm.rs`** (Additional tests):
   - `test_derive_internal_create_address_formula` - Verifies TRON's keccak256(txid||nonce) formula
   - `test_derive_internal_create_address_returns_none_without_txid` - Edge case handling
   - `test_increment_internal_nonce` - Nonce increment and saturation
   - `test_multiple_calls_increment_nonce` - Multiple CALLs affect nonce correctly
   - `test_tron_vs_ethereum_create_address_differs` - TRON vs ETH derivation comparison


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
- [ ] Add tests:
  - [ ] "create contract persists version=1 when allow_tvm_compatible_evm=1"
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
- [ ] Add conformance tests:
  - [ ] Constructor that does `CALL` then `CREATE` and asserts the created sub-contract address matches Java fixture.
  - [ ] Multiple internal creates in one tx (address sequence stable).
  - [ ] Multiple txs from same contract that do CREATE (no address collisions across txs).

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
- [ ] Add tests:
  - [ ] token transfer applied on success
  - [ ] token transfer rolled back on REVERT/OOG (natural since Trc10Change not emitted on failure)
  - [ ] Java-parity error messages for insufficient token balance / missing asset

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

- [ ] Ensure CreateSmartContract fails when owner account does not exist (even when callValue==0), with Java-parity error text.
- [ ] Confirm address validity expectations for owner/origin for remote mode:
  - [ ] If Java remote path already validated, document it; otherwise mirror `DecodeUtil.addressValid`.
- [ ] Re-evaluate the “disallow precompile address creation” check for TRON parity (either remove or gate behind a compatibility flag).

## 6) Verification steps

- [x] Rust:
  - [x] `cargo build --release` - Compiles successfully
  - [ ] `cd rust-backend && cargo test` - Run unit tests
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
   - Added unit tests: `internal_nonce_does_not_count_tx_entry_call`, `internal_nonce_skips_precompile_calls`


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

- [ ] In `rust-backend/crates/core/src/service/mod.rs` (`persist_smart_contract_metadata`):
  - [ ] Read `ALLOW_TVM_COMPATIBLE_EVM` from dynamic properties (getter already exists on the storage adapter).
  - [ ] If enabled, set `smart_contract.version = 1` before `put_smart_contract(...)`.
  - [ ] If disabled, ensure `smart_contract.version` is `0` (or leave as-is if the wire value is already `0`).
- [ ] Add tests:
  - [ ] “create contract persists version=1 when allow_tvm_compatible_evm=1”
  - [ ] TransferContract validation against the newly created contract rejects with Java message when version==1 (end-to-end parity check).

## 2) Implement TRON legacy `CREATE` address derivation (txid + internal nonce)

Goal: match Java internal `CREATE` address derivation:

- `TransactionUtil.generateContractAddress(rootTxId, nonce)` → `keccak256(txid || nonce_be_u64)[12..]`
- `nonce` is a **global per-root-tx internal-transaction counter** incremented by calls as well.

Checklist:

- [ ] Extend `TronExternalContext` to carry:
  - [ ] `root_txid: Option<B256>`
  - [ ] `internal_tx_nonce: u64` (init to 0 each top-level tx)
- [ ] Set `root_txid` during `TronEvm::setup_environment()` from `ExecutionContext.transaction_id`.
- [ ] Override address derivation for **every** legacy `CREATE` (not just the first/top-level), using:
  - [ ] top-level CreateSmartContract: still uses `txid + owner_address` (WalletUtil scheme)
  - [ ] internal CREATE opcode: uses `txid + internal_tx_nonce` (TransactionUtil scheme)
- [ ] Implement `internal_tx_nonce` increments to match Java ordering:
  - [ ] Increment on CALL-start events (since Java increments for internal CALL transactions).
  - [ ] For CREATE: derive address using current nonce, then increment (Java derives using current nonce, increments later).
  - [ ] Confirm whether to count precompile calls the same way Java does (java-tron creates internal tx records for many calls).
- [ ] Add conformance tests:
  - [ ] Constructor that does `CALL` then `CREATE` and asserts the created sub-contract address matches Java fixture.
  - [ ] Multiple internal creates in one tx (address sequence stable).
  - [ ] Multiple txs from same contract that do CREATE (no address collisions across txs).

## 3) Apply TRC-10 `call_token_value` transfer for CreateSmartContract

Goal: match Java `MUtil.transferToken(...)` behavior for contract creation when `ALLOW_TVM_TRANSFER_TRC10 == 1`.

- [ ] Decide the rollback model:
  - [ ] Token transfer must be reverted if contract creation reverts/halts.
  - [ ] Prefer integrating into the same write buffer / journal as EVM state so failure discards changes.
- [ ] Implement pre-execution token transfer for CreateSmartContract:
  - [ ] Decode CreateSmartContract proto from tx.data.
  - [ ] Compute the created contract address deterministically (txid+owner) before execution.
  - [ ] Perform the token transfer owner → contract account (after ensuring the contract account exists for the transfer semantics).
  - [ ] Emit `Trc10Change` so the Java side (or Rust persistence) can mirror the update deterministically.
- [ ] Add tests:
  - [ ] token transfer applied on success
  - [ ] token transfer rolled back on REVERT/OOG
  - [ ] Java-parity error messages for insufficient token balance / missing asset

## 4) Match Java energy/resource capping for contract creation

Goal: approximate `VMActuator.getAccountEnergyLimitWithFixRatio(...)` (and fork-specific rules):

- availableEnergy = leftFrozenEnergy + max(balance - callValue, 0) / energyFee
- energyFromFeeLimit = feeLimit / energyFee
- energyLimit = min(availableEnergy, energyFromFeeLimit)

Checklist:

- [ ] Implement “available energy” computation in Rust using:
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

- [ ] Rust:
  - [ ] `cd rust-backend && cargo test`
  - [ ] Run any conformance runner cases that include CreateSmartContract + internal CREATE patterns
- [ ] Java:
  - [ ] `./gradlew :framework:test`
  - [ ] Run dual-mode tests if available: `./gradlew :framework:test --tests "org.tron.core.storage.spi.DualStorageModeIntegrationTest"`


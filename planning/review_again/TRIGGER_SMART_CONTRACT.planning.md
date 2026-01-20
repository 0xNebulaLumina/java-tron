# TRIGGER_SMART_CONTRACT parity review (rust-backend vs java-tron)

## Scope / sources reviewed

- **Rust backend**
  - `rust-backend/crates/execution/src/lib.rs` (`ExecutionModule::execute_transaction_with_storage`, `validate_trigger_smart_contract`)
  - `rust-backend/crates/execution/src/tron_evm.rs` (`setup_environment` TriggerSmartContract branch)
- **Java “source of truth” logic**
  - `actuator/src/main/java/org/tron/core/actuator/VMActuator.java` (`call()`, `checkTokenValueAndId`, callValue/tokenValue transfers)
  - `actuator/src/main/java/org/tron/core/vm/program/invoke/ProgramInvokeFactory.java` (how TVM gets `callValue` + `data`)
- **Java request builders (important for remote execution)**
  - Conformance fixture generator: `framework/src/test/java/org/tron/core/conformance/VmTriggerFixtureGeneratorTest.java` (`buildTriggerRequest`)
  - Production remote SPI: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java` (`case TriggerSmartContract`)

## Java baseline behavior (VMActuator + ProgramInvokeFactory)

At a high level, Java treats `TriggerSmartContract` as:

- **Inputs** (from `TriggerSmartContract` proto):
  - `owner_address` (caller/origin)
  - `contract_address` (callee)
  - `call_value` (TRX value)
  - `data` (EVM calldata)
  - `call_token_value` + `token_id` (TRC-10 value, gated by `ALLOW_TVM_TRANSFER_TRC10`)
- **Validation / gating** (simplified):
  - VM must be enabled (`supportVM()`)
  - target must exist in contract store (`getContract(...) != null`) or it throws `"No contract or not a smart contract"`
  - `feeLimit` must be within `[0, maxFeeLimit]`
  - `checkTokenValueAndId(tokenValue, tokenId)`
  - (fork-dependent) `callValue >= 0` and `tokenValue >= 0` are enforced only when `StorageUtils.getEnergyLimitHardFork()` is enabled
- **Pre-execution transfers**:
  - if `callValue > 0`: `MUtil.transfer(...)` from caller → contract
  - if `ALLOW_TVM_TRANSFER_TRC10` and `tokenValue > 0`: `MUtil.transferToken(...)` from caller → contract
- **Execution**:
  - TVM gets `callValue` and `data` from the `TriggerSmartContract` proto (see `ProgramInvokeFactory`).

## What Rust does today for TRIGGER_SMART_CONTRACT

### ExecutionModule validation + prechecks

In `rust-backend/crates/execution/src/lib.rs`:

- If `contract_type == TriggerSmartContract`:
  - It checks `tx.to` points to an existing smart contract (via `tron_has_smart_contract` or `get_code` fallback).
  - It calls `validate_trigger_smart_contract(...)`, which **decodes `TriggerSmartContract` from `tx.data`** and validates:
    - VM enabled (`ALLOW_CREATION_OF_CONTRACTS == 1`)
    - owner address format + account existence
    - contract address present + parseable
    - `feeLimit` bounds (`tx.gas_limit` vs `MAX_FEE_LIMIT`)
    - callValue balance sufficiency (only when `trigger.call_value > 0`)
    - TRC-10 checks (only when enabled and `trigger.call_token_value > 0`)

Then it converts feeLimit(SUN) → gas/energy units via `gas_limit /= energy_fee_rate` (dynamic `ENERGY_FEE`).

### EVM environment mapping

In `rust-backend/crates/execution/src/tron_evm.rs`, for `contract_type == TriggerSmartContract`:

- It **tries** to decode `TriggerSmartContract` from `tx.data`.
  - If decoding succeeds, it sets `env.tx.data = trigger.data` (calldata).
  - If `trigger.call_value < 0`, it sets `disable_balance_check = true` to allow execution to proceed for “negative callValue” fixtures.

## Critical mismatch: what bytes are in `TronTransaction.data`?

There are **two different Java-side encodings in this repo**:

### A) Conformance fixtures (matches Rust’s assumption)

`framework/src/test/java/org/tron/core/conformance/VmTriggerFixtureGeneratorTest.java` builds the request with:

- `TronTransaction.data = triggerContract.toByteArray()` **(full `TriggerSmartContract` proto bytes)**

This matches Rust’s current behavior, which expects to decode `TriggerSmartContract` from `tx.data`.

### B) Production RemoteExecutionSPI (does NOT match Rust’s assumption)

`framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java` currently maps:

- `TronTransaction.to = triggerContract.getContractAddress()`
- `TronTransaction.data = triggerContract.getData()` **(calldata only, *not* the proto)**
- and separately sets `TronTransaction.contract_parameter = contract.getParameter()` (raw Any carrying the full `TriggerSmartContract` proto)

But Rust:

- ignores `metadata.contract_parameter` entirely today, and
- `validate_trigger_smart_contract(...)` hard-decodes from `tx.data`.

**Practical impact**

- With the production RemoteExecutionSPI format, Rust will fail validation at:
  - `Failed to decode TriggerSmartContract: ...`
- Any TriggerSmartContract-specific parity logic that requires decoding (token checks, `call_value < 0` handling) is effectively unreachable under the production format.

## Additional parity gaps (even after fixing the encoding mismatch)

These are not strictly “format” issues:

- **Energy-limit hardfork semantics**: Java gates `callValue>=0` and `tokenValue>=0` checks behind `StorageUtils.getEnergyLimitHardFork()`. Rust trigger validation currently doesn’t model this fork gate (it only conditionally checks `> 0` cases), so post-hardfork negative-value semantics can diverge.
- **TRC-10 call_token_value transfer**: Java transfers TRC-10 token value into the contract before execution when enabled. Rust currently validates token balance/asset existence, but I did not find corresponding “pre-execution token transfer” state changes for TriggerSmartContract in the Rust EVM path.
- **Energy limit calculation**: Java’s effective TVM energy limit is `min(availableEnergy, feeLimit/energyFee)` with creator/caller split logic. Rust uses the feeLimit/energyFee conversion but does not appear to enforce `availableEnergy` constraints; that can change “success vs out-of-energy” behavior in low-resource edge cases.

## Conclusion

Rust’s TRIGGER_SMART_CONTRACT implementation is internally consistent with the **conformance fixture request format** (where `tx.data` carries the full TriggerSmartContract proto), but it **does not match the current production RemoteExecutionSPI request format** (where `tx.data` is calldata and the proto lives in `contract_parameter`).

In other words: **Rust matches one Java-side builder (fixtures), but not the other (RemoteExecutionSPI)**.

## Recommendation (architecture choice)

Pick one (or support both):

1) **Standardize Java RemoteExecutionSPI to match fixtures**: send `TriggerSmartContract.toByteArray()` in `TronTransaction.data` (and keep calldata extraction on the Rust side).

2) **Make Rust accept both encodings**: prefer decoding from `metadata.contract_parameter.value` when present and type_url matches TriggerSmartContract; otherwise fallback to `tx.data` (for fixture compatibility).


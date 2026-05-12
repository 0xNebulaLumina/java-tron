# Close Loop — 5.1 TriggerSmartContract TRC-10 Pre-Execution Transfer

This file closes Section 5.1 of `close_loop.todo.md`. The current
known gap (Rust rejects `TriggerSmartContract` with
`tokenValue > 0` because it does not yet implement Java's
pre-execution `MUtil.transferToken` step) needs to be **either
closed by implementing the design below, OR explicitly kept out of
the `RR` whitelist target until the implementation lands**. This
audit takes the second option for Phase 1: it freezes the design,
but leaves the Rust handler in its existing reject path so no
silent state divergence can leak through.

Companion notes:

- `close_loop.contract_matrix.md` — `TriggerSmartContract` is
  tagged `RR blocked` because of this gap.
- `close_loop.sidecar_parity.md` — TRC-10 sidecar gaps that
  intersect with this work.
- `close_loop.energy_limit.md` — energy accounting that interacts
  with the pre-transfer.
- `planning/review_again/TRIGGER_SMART_CONTRACT.todo.md` — the
  precursor todo that left this item as a `(Future)` follow-up.

## Java reference behavior

The behavior we need to mirror lives in
`actuator/.../org/tron/core/actuator/VMActuator.java` around line 549:

```java
program.getResult().setContractAddress(contractAddress);
// transfer from callerAddress to targetAddress according to callValue
if (callValue > 0) {
  MUtil.transfer(rootRepository, callerAddress, contractAddress, callValue);
}
if (VMConfig.allowTvmTransferTrc10() && tokenValue > 0) {
  MUtil.transferToken(rootRepository, callerAddress, contractAddress,
      String.valueOf(tokenId), tokenValue);
}
```

`MUtil.transferToken` itself is in
`actuator/.../org/tron/core/vm/utils/MUtil.java:43`:

```java
public static void transferToken(Repository deposit, byte[] fromAddress,
    byte[] toAddress, String tokenId, long amount)
    throws ContractValidateException {
  if (0 == amount) {
    return;
  }
  VMUtils.validateForSmartContract(deposit, fromAddress, toAddress,
      tokenId.getBytes(), amount);
  deposit.addTokenBalance(toAddress,   tokenId.getBytes(),  amount);
  deposit.addTokenBalance(fromAddress, tokenId.getBytes(), -amount);
}
```

Key facts:

1. The transfer happens **before** the VM is invoked — it is part
   of the `play()` setup, not part of the EVM call itself. This
   means the VM sees the post-transfer balances on entry.
2. The transfer is gated on `tokenValue > 0` AND
   `VMConfig.allowTvmTransferTrc10()` (= the
   `ALLOW_TVM_TRANSFER_TRC10` dynamic property). When either is
   false, no transfer occurs and the VM sees the pre-transfer
   (= unchanged) balances.
3. `validateForSmartContract` throws `ContractValidateException`
   on invalid inputs (missing asset, insufficient balance,
   non-existent recipient if applicable). The throw happens
   before any balance mutation.
4. The transfer is two `addTokenBalance` calls in opposite
   directions, NOT a single atomic helper. They share the same
   `Repository`, so they observe each other inside the same
   transaction commit.
5. There is no explicit rollback path in `transferToken` itself
   for "VM revert after transfer". The Java side relies on the
   surrounding `Repository` snapshot — when the VM reverts, the
   commit gates inside `VMActuator.java:235` (the `play` /
   `commit` path) and `Program.java:1049` (the per-Program revert
   handling) decide not to call `rootRepository.commit()`, so the
   token transfer is reverted as part of the same commit boundary
   as the VM state.

6. `MUtil.transferToken` itself just delegates the validation to
   `VMUtils.validateForSmartContract`. That validator (see
   `actuator/.../org/tron/core/vm/VMUtils.java:201`) explicitly
   rejects `Arrays.equals(ownerAddress, toAddress)` with the
   error string `"Cannot transfer asset to yourself."`. There is
   no net-zero self-transfer "fast path" — Java unconditionally
   throws.

## Phase 1 design (Rust pre-execution token transfer)

This is the design the Rust handler will implement when the gap
is closed. **It is NOT implemented in Phase 1.** Implementation
is tracked as an explicit follow-up in the contract matrix and
in `close_loop.todo.md` Section 5.1.

### Where the transfer fits in the existing Rust call path

The transfer must execute on the same path as
`tron_evm.rs::execute_transaction_with_state_tracking` for
`TriggerSmartContract` (after `validate_trigger_smart_contract`
passes, before `evm.execute(...)` is invoked). The current reject
path in `lib.rs:523-538` is the exact spot where the new
implementation replaces the early-return.

Required surfaces on the storage adapter (some already exist —
audit during implementation):

- `tron_get_asset_balance_v2(owner, token_id_bytes)` — already
  used by `validate_create_smart_contract`, so it exists.
- `tron_set_asset_balance_v2(owner, token_id_bytes, new_balance)`
  or an `add_token_balance` equivalent — needs verification.
- `tron_get_asset_issue(token_id_bytes, allow_same_token_name)` —
  exists per `validate_trigger_smart_contract`.

### Pre-transfer validation

Run BEFORE buffering any balance change. The validations mirror
Java's `VMUtils.validateForSmartContract` and the
`MUtil.transferToken` `0 == amount` short-circuit. Importantly,
the existing `validate_trigger_smart_contract` already enforces
the negative-value and tokenId-zero gates with Java's exact
gating, so the pre-transfer hook should NOT re-add stricter
checks — it should only run the additional checks that
`MUtil.transferToken` runs at call time (which `validate_…` does
not duplicate):

1. If `tokenValue == 0`: return `Ok(())` immediately. No
   transfer. No state change. This matches the Java
   `if (0 == amount) return;` short-circuit. (Note: the existing
   `validate_trigger_smart_contract` already short-circuits
   `(token_value, token_id) := (0, 0)` when
   `ALLOW_TVM_TRANSFER_TRC10 == 0`, so this branch also covers
   the dynamic-property-disabled case.)
2. **Self-transfer check**: if `caller_address == contract_address`,
   return `"Cannot transfer asset to yourself."`. This mirrors
   `VMUtils.java:201` and is the gate the existing
   `validate_trigger_smart_contract` does NOT cover. Without
   this, Phase 2 would silently accept self-transfers that
   Java rejects.
3. Look up the asset issue row for `tokenId`. If absent, return
   `"No asset !"`. (Same string as Java for receipt
   compatibility.) Note: `validate_trigger_smart_contract`
   already does this check, so re-running it here is defensive
   but harmless; either way the error string and outcome match.
4. Read the caller's asset balance. If `<= 0`, return
   `"assetBalance must greater than 0."`. Same defensive
   re-check.
5. If `caller_asset_balance < tokenValue`, return
   `"assetBalance is not sufficient."`. Same defensive re-check.
6. Verify the recipient (the contract address) exists. If not,
   the existing trigger-validation code path already errors out
   earlier with `"No contract or not a smart contract"`. No new
   check is needed here.

The validations Phase 2 must NOT re-add (because they belong to
the existing `validate_trigger_smart_contract` and have hardfork
/ committee-flag gating that the pre-transfer hook would
double-apply if it ran them again):

- `tokenValue >= 0`. Java only enforces `tokenValue >= 0` when
  the energy-limit hardfork is active; see
  `actuator/.../VMActuator.java:486`.
- `tokenId > MIN_TOKEN_ID && tokenId != 0`. Java only enforces
  this when both `ALLOW_TVM_TRANSFER_TRC10` and `ALLOW_MULTI_SIGN`
  are enabled; see `actuator/.../VMActuator.java:655`. The
  `validate_trigger_smart_contract` Rust path mirrors this
  gating exactly.

Phase 2 must reuse the existing validation path's behavior for
those two gates rather than duplicating the checks at the
pre-transfer hook, otherwise Phase 2 would reject replays that
Java currently accepts (when one of the dynamic-property gates
is off).

### State mutation (after validation passes)

Mutate two balances in the storage adapter. Both writes MUST go
through the **same VM write buffer / overlay that wraps the rest
of the trigger's VM execution**, so that the buffer's
commit-vs-discard machinery covers both the pre-transfer and the
VM body together:

```text
caller_balance     := caller_balance     - tokenValue
recipient_balance  := recipient_balance  + tokenValue
```

The buffer in question is `EngineBackedEvmStateStore::new_with_buffer`
(`crates/execution/src/storage_adapter/engine.rs:138`). It is
attached to VM execution today only when `rust_persist_enabled
== true`, per `crates/core/src/service/grpc/mod.rs:1431`. The
**compute-only RR profile** (`rust_persist_enabled == false`)
currently runs VM trigger execution against the unbuffered
adapter — there is no equivalent "iter-3 transaction buffer"
covering this path, contrary to what an earlier draft of this
file claimed. See the rollback section below for the full
explanation; the short version is that Phase 2 must add the
buffer to compute-only VM execution before the pre-transfer
writes are safe to enable in that profile.

This MUST be a debit-then-credit on the same `tokenId` key. The
Java code happens to do credit-then-debit (`addTokenBalance(to,
amount)` followed by `addTokenBalance(from, -amount)`), but the
order is irrelevant because both happen against the same in-memory
`Repository` snapshot. Rust's order can be either way — the only
invariant is that both mutations land in the same buffer flush as
the VM execution that follows them, and that the buffer is
discarded together with the VM state on revert / halt.

### Rollback semantics

The pre-transfer is an unconditional state mutation. If anything
later in the call path fails, the mutation must be undone. The
**mechanism does not exist for VM trigger execution today** —
this is a real Phase 2 prerequisite, not a "rides on the
existing buffer" cleanup. Concretely:

- `crates/core/src/service/grpc/mod.rs:1431` only attaches the
  `EngineBackedEvmStateStore` write buffer when
  `rust_persist_enabled == true` OR `tx_kind == NonVm`. For VM
  TriggerSmartContract execution in the **compute-only RR
  profile** (`rust_persist_enabled == false`), no buffer is
  attached at all and writes flow straight through to the
  underlying storage engine. There is nothing to discard on
  revert because nothing is buffering.
- `EngineBackedEvmStateStore::new_with_buffer` (see
  `crates/execution/src/storage_adapter/engine.rs:138`) is the
  buffered constructor; the unbuffered `::new` constructor is
  what compute-only VM execution gets.

Phase 2 implementation must therefore add a per-tx overlay
(either by always attaching the write buffer for VM execution,
or by introducing a dedicated TRC-10 pre-transfer journal that
the existing reject path can be replaced with safely). Without
that overlay, the pre-transfer is unrollback-able in compute-only
RR mode and a VM revert silently leaks the token movement to
disk.

Once the overlay exists, the rollback contract is:

- The pre-transfer happens AFTER validation (so validation
  failures cannot leave a half-applied state).
- The pre-transfer happens INSIDE the same write buffer as the
  VM execution.
- If the VM reverts (`TronExecutionResult.success == false` and
  `error == "Call reverted"`), the buffer-discard path
  in `tron_evm.rs` rolls back the transfer along with everything
  else the VM touched.
- If the VM halts (`error == "Call halted: ..."`), same: the
  buffer discards.
- If the VM succeeds (`success == true`), the buffer commits and
  the transfer becomes visible alongside the rest of the VM
  state changes.

This matches Java's behavior exactly: Java's `Program` records
the transfer in the `Repository` snapshot, and the commit gates
inside `VMActuator.java:235` and `Program.java:1049` decide
whether `rootRepository.commit()` is called based on whether the
VM produced a revert / halt.

**Critical invariant**: the pre-transfer MUST be enclosed in a
buffer that supports commit-vs-discard. Performing the transfer
through any path that bypasses the buffer (e.g. direct
`storage_adapter.set_*` without going through a buffered
adapter) would leak the mutation on revert. The reject path
currently in place is *more conservative* than the buffered
implementation would be, which is why this design recommends
keeping the reject path until the buffer is wired through for
VM execution paths in compute-only RR mode.

In the canonical RR profile (`rust_persist_enabled == true`)
the buffer IS attached for VM execution, so the rollback story
holds there today; the gap is specifically the compute-only
profile.

### Energy accounting

Java's `MUtil.transferToken` does NOT charge any explicit energy
cost for the transfer itself — the transfer is "free" in energy
terms; the cost is amortized into the VM call's bandwidth
accounting. The Rust implementation MUST match this:

- Do NOT increment `energy_used` on the
  `TronExecutionResult` for the transfer itself.
- Do NOT increment `bandwidth_used` for the transfer itself; the
  enclosing `TriggerSmartContract` bandwidth charge covers the
  whole transaction.
- The VM execution that follows the transfer sees the updated
  balances on its starting context, but there is no energy
  difference between "VM started with balance X-tokenValue" vs
  "VM started with balance X" — the VM gas/energy accounting is
  per-opcode, not per-balance-snapshot.

### Sidecar emission

The pre-transfer mutates two TRC-10 asset balances, which are
state changes that need to round-trip back to Java for parity.
Two emission paths exist on paper, but only one is viable today:

1. **`AccountChange` in S1 `state_changes`** — **non-viable**.
   The S1 wire types (`AccountInfo` and `AccountChange` in
   `framework/src/main/proto/backend.proto:725`) carry
   `address`, `balance`, `nonce`, `code_hash`, `code`, and the
   AEXT resource-usage fields, but they do NOT carry any TRC-10
   `asset` map. The Rust execution side
   (`crates/execution/src/tron_evm.rs:517` and surrounding code)
   has the same shape — there is no field for "this account's
   TRC-10 token X balance changed from A to B". Forcing the
   pre-transfer into S1 would require a proto change to add
   asset maps to `AccountInfo`, which is Phase 2+ proto-shape
   work outside this design's scope. Mark this option closed.

2. **`Trc10Change.AssetTransferred` in S4** — the only viable
   path. The Rust handler emits a `Trc10AssetTransferred` row
   with `{owner_address: caller, to_address: contract_address,
   asset_name | token_id, amount: tokenValue}` and relies on
   Java's `applyTrc10Changes` to update the asset maps. This
   matches how `TransferAssetContract` already works today, so
   the Java side already has the apply machinery in place.

   **Required**: emit S4 `Trc10Change.AssetTransferred`. The
   pre-transfer hook MUST populate `trc10_changes` rather than
   trying to encode the balance delta inside `state_changes`.
   There is no "option 1 vs option 2" choice — option 1 is closed
   as non-viable by the proto/wire shape.

### Edge cases the design must cover

- **`tokenValue == 0`**: no transfer, no validation, no sidecar
  emission. Same as Java's `if (0 == amount) return;` early
  return.
- **`tokenValue > 0` but `ALLOW_TVM_TRANSFER_TRC10 == 0`**: the
  existing `validate_trigger_smart_contract` already coerces
  `(token_value, token_id)` to `(0, 0)` in this case, so by the
  time the pre-transfer hook runs, `tokenValue == 0` and the
  short-circuit above kicks in.
- **Self-transfer** (caller_address == contract_address):
  **rejected**. `MUtil.transferToken` delegates validation to
  `VMUtils.validateForSmartContract`, which throws
  `"Cannot transfer asset to yourself."` at
  `actuator/.../VMUtils.java:201` before any balance mutation.
  The Rust pre-transfer hook MUST run the same check (see
  validation step #2 above) and return the same error string,
  with no buffer mutation and no `Trc10Change` row emitted.
  An earlier draft of this section said "perform the credit
  and debit on the same address with a net effect of zero" —
  that was wrong; Java unconditionally rejects.
- **Caller account does not exist**: validation already errors
  out earlier (`"Validate InternalTransfer error, no
  OwnerAccount."` from `validate_create_smart_contract` and
  similar from `validate_trigger_smart_contract`).
- **Recipient (contract) account does not exist**: validation
  already errors out earlier with `"No contract or not a smart
  contract"`.
- **Insufficient asset balance**: validation already covers this
  case (#5 and #6 above). The transfer hook does not need to
  re-check.
- **VM revert after pre-transfer**: rollback via the buffer
  commit-vs-discard machinery; documented above.
- **VM halt (out-of-energy / invalid opcode) after pre-transfer**:
  same rollback path as revert.
- **VM success after pre-transfer**: buffer commits; transfer
  becomes visible.
- **Sidecar emission in revert/halt cases**: the `Trc10Change`
  row should NOT be emitted on revert/halt. The Rust handler
  must populate `trc10_changes` only AFTER successful VM
  execution / `TronExecutionResult` formation, NOT in the same
  block as the buffered balance writes. The outer buffer commit
  happens later in the gRPC handler (see
  `crates/core/src/service/grpc/mod.rs:1588` and the surrounding
  `tron_transaction_result` assembly at line ~1666); waiting
  for an "observed commit" from outside the execution function
  is not how the pipeline is structured. The right rule is:
  build the sidecar row inside the same `Ok(...)` arm that
  produces the success-shaped `TronExecutionResult`, and skip
  it on the revert/halt arms. This keeps the sidecar consistent
  with the buffered balance writes regardless of how the outer
  gRPC handler decides to commit / fall back to Java apply via
  `write_mode = COMPUTE_ONLY`.

## Test plan

When the design is implemented (Phase 2 work, NOT Phase 1), the
test plan is:

| Case | Setup | Expected outcome |
| ---- | ----- | ---------------- |
| Happy path token pre-transfer | Caller has 1000 tokens; trigger with `tokenValue=100`; VM call succeeds | Caller asset balance = 900, recipient asset balance = 100, `Trc10Change.AssetTransferred` emitted (TRC-10 transfer is carried via S4 only — S1 `AccountChange` does not carry asset-map deltas) |
| `tokenValue == 0` | Trigger with `tokenValue=0`; VM call succeeds | No state change for asset balances, no `Trc10Change` row |
| Insufficient balance | Caller has 50 tokens; trigger with `tokenValue=100` | Validation error `"assetBalance is not sufficient."`, no buffer mutation, no sidecar emission |
| Missing asset | `tokenId` does not have an `assetIssue` row | Validation error `"No asset !"`, no buffer mutation, no sidecar emission |
| Self-transfer | Caller == contract address; `tokenValue=10` | Validation error `"Cannot transfer asset to yourself."` from `VMUtils.java:201`; no buffer mutation; no `Trc10Change` row |
| VM revert after pre-transfer | Pre-transfer succeeds, VM call REVERTs | Caller balance restored, recipient balance restored, NO `Trc10Change` row in result |
| VM halt after pre-transfer | Pre-transfer succeeds, VM call halts (out of energy) | Same as revert: balances restored, no `Trc10Change` row |
| Java parity: same trigger run on EE and RR | Real fixture replay | EE and RR produce identical asset balance maps + identical receipt + identical state digest |

The "VM revert after pre-transfer" and "VM halt after pre-transfer"
cases are the ones that would have been silently broken if the
implementation had used a non-buffered storage write — they are
the regression tests that justify the buffer-commit invariant
above.

## Phase 1 decision

For Phase 1, this design is **frozen but NOT implemented**. The
Rust handler keeps its existing explicit-reject path for
`TriggerSmartContract` with `tokenValue > 0`
(`lib.rs:523-538`). The tradeoffs are:

- **Pro**: no risk of silent state divergence on RR runs that
  would happen if a half-implemented pre-transfer hit production.
- **Pro**: `TriggerSmartContract` is correctly tagged `RR blocked`
  in `close_loop.contract_matrix.md` and is NOT on the Phase 1
  whitelist target. The reject path enforces the
  contract-matrix tag at runtime.
- **Pro**: Phase 1 acceptance for Section 5.1 is satisfied by the
  "explicitly kept out of the `RR` whitelist target" branch of
  the acceptance criterion.
- **Con**: The whitelist target cannot grow to include
  `TriggerSmartContract` until Phase 2 work lands. Any contract
  that would benefit from `RR canonical-ready` `TriggerSmartContract`
  parity is also blocked.

The reject path stays. The whitelist target stays minimal. The
design above becomes the implementation blueprint for Phase 2 or
later when the team is ready to absorb the buffer-commit rollback
risk.

## Phase 1 acceptance

Section 5.1 acceptance ("The current known gap is either closed
or explicitly kept out of the `RR` whitelist target") is
satisfied by the "explicitly kept out" branch:

- The reject path in `lib.rs:523-538` still runs for any
  `TriggerSmartContract` with `tokenValue > 0`.
- `close_loop.contract_matrix.md` tags `TriggerSmartContract`
  as `RR blocked` and the Phase 1 whitelist target
  (TransferContract + CreateSmartContract + UpdateSettingContract)
  does NOT include it.
- This file freezes the design that Phase 2 implementation will
  follow, so the work is not lost when the gap is eventually
  closed.

## Follow-up implementation items

These are tracked as Phase 2+ work, not Phase 1 deliverables:

- [ ] Replace the explicit-reject path in
      `rust-backend/crates/execution/src/lib.rs:523-538` with the
      pre-transfer implementation described above.
- [ ] Verify (or add) `tron_set_asset_balance_v2` /
      `add_token_balance` on the storage adapter so the transfer
      can be expressed in two writes against the per-tx buffer.
- [ ] Implement the S4 `Trc10Change.AssetTransferred` emission on
      the success-shaped `Ok(...)` arm of the trigger handler.
      (S1 option is already closed as non-viable — `AccountInfo`
      / `AccountChange` carries no TRC-10 asset map.)
- [ ] Add the test cases from the test plan table above as Rust
      execution tests, plus at least one EE-vs-RR replay fixture.
- [ ] Once green, flip `TriggerSmartContract` from `RR blocked`
      to `RR candidate` (NOT directly to canonical-ready — the
      contract still has other coverage gates from
      `close_loop.contract_matrix.md`).
- [ ] After replay parity is green, consider whether
      `TriggerSmartContract` belongs on the Phase 2 whitelist target.

## Anti-pattern guard

**Do not implement the pre-transfer with direct
`storage_adapter.set_*` calls outside the buffered
`EngineBackedEvmStateStore` path.** Doing so would silently leak
the mutation on VM revert / halt, which is exactly the regression
the test plan above is designed to catch. The pre-transfer must
go through the same buffer that the VM uses for its own state
changes.

**The buffer's commit-vs-discard machinery only covers the
rollback case automatically once the Phase 2 prerequisite has
landed** — namely, attaching `EngineBackedEvmStateStore::new_with_buffer`
for VM trigger execution in the compute-only RR profile. Today,
`grpc/mod.rs:1431` only attaches the buffer when
`rust_persist_enabled == true` OR `tx_kind == NonVm`. Until
that prerequisite lands, the canonical RR profile gets buffered
rollback for free but the compute-only profile does NOT, and
the reject path in `lib.rs:523-538` is the only thing keeping
compute-only mode honest. Do not enable the pre-transfer hook
in compute-only mode without first wiring through the buffer
for VM execution paths.

**Do not emit the `Trc10Change` sidecar before the VM call.**
The sidecar must only land on the success-shaped
`TronExecutionResult` arm. Putting it in the result struct
before the VM runs would let a reverted transaction produce a
non-empty `trc10_changes` vector that the Java applier would
then apply to its local stores, silently transferring tokens
that the VM rolled back. Build the sidecar inside the same
`Ok(...)` arm that produces the success result; on revert /
halt arms, populate `trc10_changes: vec![]`.

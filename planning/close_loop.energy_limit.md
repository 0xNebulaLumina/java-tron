# Close Loop — 1.2 `energy_limit` Wire Semantics

This file closes Section 1.2 of `close_loop.todo.md`. It freezes the
canonical interpretation of the `energy_limit` field on the Java ↔ Rust
execution boundary. Until this file is superseded by another planning
note, producers and consumers must agree on this contract.

## Audit findings

### Java sender — production `RemoteExecutionSPI`

File: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`

- Default initialization: `long energyLimit = transaction.getRawData().getFeeLimit();`
  (i.e. fee-limit SUN).
- For `CreateSmartContract` and `TriggerSmartContract`, this default is
  overwritten with the result of `computeEnergyLimitWithFixRatio(...)`,
  which returns an already-converted value in **energy units**
  (caller side only — see the lingering `getTotalEnergyLimitWithFixRatio`
  gap below).
- `computeEnergyLimitWithFixRatio` has its own exception-handling
  fallback: if any of the helper's lookups fail (StoreFactory null,
  ChainBaseManager null, AccountStore null, account not found, or any
  thrown exception during the computation), it logs a warning and
  returns the **raw fee-limit SUN** instead of an energy-unit value.
  See `RemoteExecutionSPI.java` lines 313–376. That means even on the
  VM path, today's behavior can drop back to fee-limit SUN at runtime
  without any wire-level signal — making mixed-unit failures hard to
  debug during the migration window.
- For every other contract type, the default stays — meaning the Java
  bridge sends **fee-limit SUN** on the wire for non-VM contracts.

So for production traffic:

- `CreateSmartContract` → energy units, **with raw fee-limit SUN as a
  silent exception fallback**.
- `TriggerSmartContract` → energy units, **with raw fee-limit SUN as a
  silent exception fallback**.
- All other contract types → fee-limit SUN.

This is intrinsically ambiguous and must be locked.

### Java sender — fixture generators

File: `framework/src/test/java/org/tron/core/conformance/FixtureGenerator.java`
(and VM-specific fixture generators like `VmTriggerFixtureGeneratorTest.java`)

- `FixtureGenerator` builds requests with
  `setEnergyLimit(transaction.getRawData().getFeeLimit())` — i.e. **SUN**,
  unconditionally. No call to `computeEnergyLimitWithFixRatio`.
- `VmTriggerFixtureGeneratorTest` uses `setEnergyLimit(DEFAULT_FEE_LIMIT)` or
  `setEnergyLimit(feeLimit)` — also SUN.

So fixtures send **fee-limit SUN** across the board. This is consistent
within the fixture generator but disagrees with the production VM path.

### Rust receiver — `tron-backend-execution`

File: `rust-backend/crates/execution/src/lib.rs`

```rust
let energy_fee_rate = storage.energy_fee_rate()?.unwrap_or(0);
...
if energy_fee_rate > 0 {
    adjusted_tx.gas_limit = adjusted_tx.gas_limit / energy_fee_rate;
}
```

Rust divides the incoming `gas_limit` (our `energy_limit` wire field) by
`ENERGY_FEE` to obtain energy units.

Effect of the division:

- Fixture traffic (SUN) → divided → energy units. Correct by coincidence.
- Production non-VM traffic (SUN) → divided → energy units. Dead
  information for non-VM execution, so correctness doesn't matter.
- Production VM traffic (already energy units) → divided → **under-gas**
  by a factor of `ENERGY_FEE`. Real bug, silently corrupting VM limits.

Before this planning chunk, that file contained a `TODO: Lock the wire
spec for energy_limit` with three candidate options. After the chunk,
the comment was rewritten to a "FUTURE-LOCKED" form that records the
decision below as the target while still describing the live division
behavior — see the current `lib.rs` comment for wording.

### Conformance runner

File: `rust-backend/crates/core/src/conformance/runner.rs`

Consumes `tx.energy_limit` and maps it into `TronTransaction::gas_limit`
directly (zero or raw value). The downstream Rust execution path then
divides as above, so the runner inherits whatever semantics the fixture
encodes.

### `backend.proto`

File: `framework/src/main/proto/backend.proto`

Before this planning chunk, the comment on `int64 energy_limit = 5;`
flagged the same mismatch and pointed at `lib.rs` without any lock or
producer guidance. After the chunk it carries the "FUTURE-LOCKED"
contract description and an explicit list of current live behavior so
nobody mistakes the recorded decision for an already-shipped invariant.

## Decision

**Future-locked wire contract: `energy_limit` will be expressed in
energy units.** This is the recorded Phase 1 decision. The
producer/consumer migration to that contract has not landed yet — see
the "Audit findings" section above and the open follow-ups below for
exactly what the live system is doing today.

Once the migration lands:

- Java sender (production + fixtures) will convert fee-limit SUN into
  energy units before populating the field.
- Rust receiver will treat the field as energy units and will not
  divide by `ENERGY_FEE` again.
- For contract types that do not consume energy (non-VM contracts), the
  field will be set to `0`. Rust will tolerate `0` and fall back to a
  safe internal default (block gas limit) when the contract type does
  not need a VM-side energy cap.

Rejected alternatives:

- **Send SUN, convert in Rust.** Would push
  `getTotalEnergyLimitWithFixRatio` (caller + creator split, frozen-energy
  lookup, dynamic-property reads) into the Rust execution crate. That is
  a duplication of Java actuator logic we are trying to keep on the Java
  side for now, and we do not have the Rust surfaces in place to do it
  correctly for `CreateSmartContract`.
- **Proto flag for mixed old/new interpretations.** Unnecessary permanent
  complexity. The single-commit transition strategy below avoids the
  need for a backwards-compatibility flag.

## Migration impact

| Area                          | Change                                                                 |
| ----------------------------- | ---------------------------------------------------------------------- |
| Java `RemoteExecutionSPI`     | Use `computeEnergyLimitWithFixRatio` (or `0`) for every contract type. Non-VM paths should set `energy_limit = 0` instead of defaulting to `feeLimit`. |
| Java fixture generators       | Stop sending `feeLimit` as `energyLimit`. For VM fixtures, compute `feeLimit / energyFee` (or the cap). For non-VM fixtures, set `0`. |
| Rust execution                | Remove `adjusted_tx.gas_limit = adjusted_tx.gas_limit / energy_fee_rate` in `execution/src/lib.rs`. Treat the field as energy units. |
| Conformance runner            | No change to mapping logic; only the meaning of the value changes.     |
| `backend.proto`               | Update comment on `energy_limit` to state the energy-units contract and cross-reference this file. |
| EE-vs-RR comparison tooling   | Consumed values become smaller by a factor of `energy_fee_rate` for old fixture runs. Re-generate fixtures under the new contract before citing parity. |
| Replay tooling                | Same: replay must regenerate its request shapes to match new contract. |
| `getTotalEnergyLimitWithFixRatio` gap | Caller/creator split for `CreateSmartContract` is still only partially implemented on the Java bridge. Tracked as follow-up, independent of the wire-unit lock. |

## Transition strategy

1. Land the decision doc (this file) and `backend.proto` comment update
   together so the intent is recorded before any code change.
2. Plan the code migration as a **single commit** that:
   - Removes the Rust division.
   - Updates Java fixture generators to emit energy units (or `0`).
   - Updates Java `RemoteExecutionSPI` non-VM defaults to `0`.
   - Regenerates any committed fixtures that would otherwise read stale
     values.
3. Before merging, run:
   - `cargo test -p tron-backend-core -- --nocapture` covering VM and
     non-VM conformance fixtures.
   - `./gradlew :framework:test --tests "*FixtureGenerator*"` as a
     smoke check.
4. After the code change lands, this file's "follow-up" list below should
   shrink to empty.

If the code migration has to land in pieces, we use a **temporary guard
assertion** on the Rust side instead of a proto flag: reject incoming
`energy_limit` values that are obviously SUN-scale (e.g. > 1e9 energy
units per transaction is implausible given typical TRON blocks). This
gives us a loud failure during the transition window without adding a
permanent proto field.

## Definition of "locked"

The wire contract is considered locked when:

- `backend.proto` comments and this file agree.
- `lib.rs` comment agrees and the division has been either removed or
  marked with a unit-consistent guard assertion.
- `RemoteExecutionSPI` and fixture generators agree that they are sending
  energy units or `0` — never fee-limit SUN.
- A follow-up todo exists in Section 1.2 for the actual code migration
  if not landed in the same chunk.

## Follow-ups tracked from this decision

These are tracked as open implementation items, independent of the
"lock" itself:

- [ ] Remove the divide-by-`energy_fee_rate` line in
      `rust-backend/crates/execution/src/lib.rs` after synchronizing all
      producers.
- [ ] Update `FixtureGenerator.java` and VM-specific fixture generators
      so they produce energy-unit values (or `0` for non-VM).
- [ ] Set `energy_limit = 0` for non-VM contract types in
      `RemoteExecutionSPI.java` (purely for clarity; functionally
      Rust should ignore it).
- [ ] Extend `computeEnergyLimitWithFixRatio` to include the
      creator/caller split for `CreateSmartContract`, matching
      `VMActuator.getTotalEnergyLimitWithFixRatio`.
- [ ] Add a temporary guard assertion in Rust that fails loudly when
      incoming `energy_limit` is implausibly large (SUN-scale), so a
      partial migration is caught at runtime.

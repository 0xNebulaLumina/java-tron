# Close Loop — Phase 1 Scope Freeze

This note freezes the scope, modes, non-goals, critical path, and exit criteria for
the current "close the execution + storage loop" phase. It is the authoritative
reference for every item in `planning/close_loop.todo.md` Section 0, Section 7,
and Section 8. Later planning documents, code defaults, and config files must not
contradict this note without first updating it.

## 1. Frozen scope

Phase 1 is limited to three things:

1. Execution semantics closure.
2. Storage semantics closure.
3. `EE`-vs-`RR` parity verification.

Anything outside those three is out of scope for Phase 1, including work that
might otherwise be "nice to have".

## 2. Frozen modes

Only two strategic modes are supported as targets in this phase:

- `EE`: embedded execution + embedded storage. Baseline; the current java-tron
  behavior. Canonical write owner is the Java path.
- `RR`: remote execution + remote storage. Target; Rust owns both execution
  and storage. Canonical write owner is the Rust path.

Explicitly not target modes in this phase:

- `SHADOW`: in-process shadow comparison. Treated as legacy tooling.
- Mixed: remote execution + embedded storage, or embedded execution + remote
  storage. Not a strategic target, must not be optimized, and the roadmap
  should not be driven from how mixed modes behave.

## 3. Frozen non-goals

None of the following are started inside Phase 1:

- Rust P2P networking rewrite.
- Rust sync scheduler / peer manager rewrite.
- Rust consensus scheduling rewrite.
- Removing the Java node shell.
- Optimizing for mixed execution/storage combinations.
- Re-elevating `SHADOW` as the primary acceptance mechanism.

## 4. Frozen next milestone

After Phase 1 exits, the intended next milestone is:

- `Rust block importer / block executor readiness` — not P2P, not consensus.

The rationale is documented in Sections 6 and 7 below.

## 5. Phase 1 exit criteria

Phase 1 is only considered closed when all of the following hold:

- Java execution read/query APIs are no longer placeholders in the `RR` path.
- Rust execution read/query APIs are either implemented or explicitly
  "unsupported" — never silently fake.
- Storage transaction semantics are real enough for execution needs
  (per-transaction buffer, atomic commit, rollback discard, documented
  read-your-writes behavior).
- Storage snapshot semantics are real, or snapshot is explicitly unavailable;
  fake success is not acceptable.
- `energy_limit` wire semantics are locked (one documented unit contract across
  Java sender, Rust receiver, and fixtures).
- Write ownership is unambiguous in `EE` and `RR`.
- A first contract whitelist reaches stable `EE`-vs-`RR` parity.
- `tron-backend-storage` crate has real tests, not `0 tests`.
- Replay + CI continuously report `EE`-vs-`RR` parity state.

## 6. Why not P2P yet

P2P is not a thin edge module in java-tron. Networking startup wires many
subsystems together, message dispatch fans out to handshake / inventory /
block / sync / PBFT / relay / fetch flows, block sync is tightly coupled to
block processing, and block processing is tightly coupled to execution,
maintenance, rewards, and consensus application.

Doing P2P before a trustworthy Rust state-transition core would combine:

- the noisiest edge of the system, and
- the most stateful core of the system,

into the same migration step. That is the highest-risk path and produces the
weakest acceptance signal. P2P stays on the Java side until the Rust block
importer is demonstrably trustworthy.

## 7. Why not `SHADOW` as the main validator

`SHADOW` runs the Java baseline and the Rust target inside the same JVM
process. That is cheap to wire up but comes with structural problems:

- Shared JVM singletons and global state bleed between baseline and target,
  so "parity" observed in shadow can still hide divergence.
- Ordering of side effects inside one process can mask interleaving bugs that
  only appear when the Rust path owns its own transaction and flush timing.
- Failure isolation is weak: a Rust-side crash or panic affects the Java
  baseline's observation, so red/green signals get confused.
- It biases the roadmap toward "make shadow green" instead of "make Rust
  trustworthy on its own".

For Phase 1 we therefore treat `SHADOW` as legacy / optional tooling and move
the primary comparison model to: run `EE`, run `RR`, compare outputs, state,
replay results, and metrics outside the in-process shadow path.

## 8. Critical path

The Phase 1 critical path, in order, is:

1. Semantic freeze (this note, plus Section 1 of the todo).
2. Execution read-path closure (Section 2).
3. Storage transaction/snapshot closure (Section 3).
4. Parity verification pipeline (Section 6).
5. Block importer readiness planning (handoff to Phase 2).

P2P / sync / consensus rewrite work is explicitly kept off the critical path.

## 9. Suggested first batch

The first batch of concrete implementation work should be:

- 1.1 Canonical write ownership.
- 1.2 `energy_limit` wire contract.
- 1.3 Storage transaction semantics.
- 1.5 Contract support matrix.
- 2.1 Java `callContract` / `estimateEnergy`.
- 3.1 `transaction_id` plumbing.

These unblock most of Sections 2, 3, 5, and 6.

## 10. Parallelization opportunities

- Java execution bridge work (Section 2.1) can run in parallel with Rust
  storage semantic work (Section 3).
- Rust execution query implementation (Section 2.2) can run in parallel with
  verification harness improvements (Section 6).
- Exactly one owner should be responsible for the semantic-freeze decisions so
  implementation work in different crates does not diverge.

## 11. Defer list (must stay deferred)

- Do not start Rust P2P handshake work.
- Do not start Rust peer/session manager work.
- Do not start Rust sync scheduler / inventory pipeline work.
- Do not start Rust consensus scheduling rewrite.
- Do not optimize for mixed execution/storage modes.
- Do not re-elevate `SHADOW` as the primary acceptance path.
- Do not treat "many system contracts already run remotely" as proof that the
  full execution problem is solved.
- Do not treat "storage CRUD works" as proof that storage semantics are solved.

## 12. Handoff condition

Phase 1 is considered handed off to Phase 2 when the next roadmap discussion
can start from "Rust state-transition engine ownership", not from "networking
looks exciting".

---

## 13. Canonical write ownership (Section 1.1)

### 13.1 Write-path matrix

| Mode | Execution produces changes in | Writes reach RocksDB via | Canonical write owner |
| ---- | ----------------------------- | ------------------------ | --------------------- |
| `EE` | Java actuators (`actuator/` + VM) | Java chainbase → embedded RocksDB | **Java** |
| `RR` | Rust execution module (VM + non-VM handlers) | Rust storage module → remote RocksDB, via the buffered adapter that commits on success and is dropped on revert | **Rust** |

### 13.2 Role of `RuntimeSpiImpl.applyStateChangesToLocalDatabase` and the sidecar appliers (`applyFreezeLedgerChanges`, `applyTrc10Changes`, `applyVoteChanges`, `applyWithdrawChanges`)

- Classification: **legacy / transitional**. Retained only to keep the former
  Shadow-era "Rust computes, Java applies" path compilable and available
  while older tests and tools are rewritten. Not a Phase 1 acceptance path.
- In `RR`, these appliers are short-circuited whenever the response sets
  `WriteMode.PERSISTED`. Under the Phase 1 RR canonical config
  (`rust_persist_enabled=true`), successful Rust commit paths return
  `PERSISTED` and the Java appliers do not run. Non-success paths (EVM
  revert, execution error, adapter commit failure, storage-lock failure)
  still return `COMPUTE_ONLY`; in those cases Java's sidecar appliers are
  the only thing that can land whatever partial/no-op state Java still
  expects, which is why the transitional path is retained rather than
  removed outright in Phase 1.
- No new callers should be added. When Phase 1 follow-up cleanup is
  scheduled, these appliers are candidates for deletion once the last
  Shadow-era tool is retired.

### 13.3 `rust_persist_enabled` policy

- `RR` canonical mode setting: `true`. This is the Phase 1 target and the
  checked-in `config.toml` value. With `true`, successful VM and non-VM
  commits persist from Rust and return `PERSISTED`; non-success paths
  (revert / execution error / commit failure / lock failure) still return
  `COMPUTE_ONLY` and the Java sidecar path is the fallback for those.
- `false` is **legacy / transitional only**. Allowed solely for:
  - running old Shadow-era conformance fixtures that rely on Java
    orchestrating writes, and
  - ad-hoc experiments that deliberately want the "Rust computes, Java
    applies" split.
- `false` is **not** an `RR` acceptance mode, not a development default going
  forward, and not a migration candidate for Phase 1 exit.
- Behavior today when the flag happens to be `false`: non-VM txs still
  buffer-and-commit from Rust (the adapter wraps all non-VM execution for
  atomicity), VM txs skip Rust commit and fall back to the Java sidecar
  appliers. This asymmetry is acknowledged, kept out of scope for Phase 1
  exit, and tracked as debt on the Phase-1 cleanup list.

### 13.4 Configuration alignment actions (completed with this freeze)

- `rust-backend/crates/common/src/config.rs`: default for
  `rust_persist_enabled` updated from `false` to `true` so code default and
  checked-in `config.toml` agree with the canonical RR policy above.
- `RemoteExecutionConfig` comment block rewritten: removed the outdated
  "Option A (Java-apply) is recommended" language, replaced with the Phase-1
  canonical ownership statement and a pointer to this note.
- `rust-backend/config.toml` `rust_persist_enabled = true` kept as-is;
  surrounding comment rewritten to stop describing the flag as "legacy /
  risk of double-write" for the canonical path, and to explicitly mark the
  `false` branch as legacy/transitional.
- `crates/core/src/conformance/runner.rs` already forces
  `rust_persist_enabled: true`; this note now explains why (conformance runs
  the `RR` canonical path, not a Shadow variant).

### 13.5 Recommended profiles

- **Safe rollout profile (Phase 1 default):** `RR` with
  `rust_persist_enabled=true`, storage snapshot treated as explicit
  unsupported rather than fake (see §1.4 follow-up in the todo), `SHADOW`
  not enabled.
- **Experimental profile (conformance / debugging only):** may temporarily
  enable `SHADOW` or flip `rust_persist_enabled=false` for legacy fixtures.
  Not a release-gate configuration; results from this profile do not count
  toward Phase 1 acceptance.

### 13.6 Future guardrail

When implementation catches up with this freeze (tracked separately on the
Phase 1 implementation list, not in this note), the Rust backend should
fail fast on unsafe combinations such as `rust_persist_enabled=false` with
no Java apply path reachable, and Java should fail fast when it sees
`WRITE_MODE_COMPUTE_ONLY` in a mode that does not carry a working applier.
For this freeze it is enough that the unsafe combinations are named and
out-of-scope.

### 13.7 Answer to "who writes the final state in this mode?"

- `EE`: Java.
- `RR`: Rust.
- `SHADOW`: out of scope for Phase 1 acceptance.
- Mixed modes (remote execution + embedded storage, etc.): out of scope for
  Phase 1; the repo should not be tuned around them.

---

## 14. `energy_limit` wire semantics (Section 1.2)

### 14.1 Audit: Java sender (production `RemoteExecutionSPI`)

- Default for all VM/non-VM txs (line ~393 of `RemoteExecutionSPI.java`):
  `energyLimit = transaction.getRawData().getFeeLimit()` — i.e. raw
  **SUN**, not energy units.
- Overridden for `CreateSmartContract` (line ~536) and `TriggerSmartContract`
  (line ~560) via `computeEnergyLimitWithFixRatio(feeLimit)`, which returns
  `min(availableEnergy, feeLimit / sunPerEnergy)` — i.e. **energy units**.
- Several fallback branches in `computeEnergyLimitWithFixRatio` (null
  `StoreFactory`, null `ChainBaseManager`, null stores, missing owner
  account, caught exception) silently return the raw `feeLimit` — i.e.
  they leak **SUN** onto the wire when the account/store context is
  unavailable.
- The final `.setEnergyLimit(energyLimit)` at lines ~1069 / ~1098 therefore
  carries a value whose unit depends on the contract type **and** on
  whether the resource-context lookup succeeded.

### 14.2 Audit: Rust receiver

- `rust-backend/crates/execution/src/lib.rs` (lines ~113-131, inside
  `execute_vm_transaction`):
  ```
  if energy_fee_rate > 0 {
      adjusted_tx.gas_limit = adjusted_tx.gas_limit / energy_fee_rate;
  }
  ```
  unconditionally divides the incoming `gas_limit` by `energy_fee_rate`,
  i.e. assumes the wire value is **SUN**.
- For production VM txs this is a **double-divide**: Java already divided
  by `sunPerEnergy`, and Rust divides again, under-gassing the tx by a
  factor of `sunPerEnergy` (typically 100).
- For conformance fixtures (runner.rs:504-508) which do send raw
  `feeLimit` (SUN), the single Rust divide happens to be correct by
  accident, masking the production regression.

### 14.3 Audit: conformance fixtures

- Fixture generators write `tx.energy_limit = feeLimit` (SUN) directly
  into the proto, as acknowledged in the `backend.proto` comment on
  `TronTransaction.energy_limit`.
- Fixture parity therefore depends on Rust doing the divide, which is
  the opposite of what the production path needs.

### 14.4 Decision: Java sends **energy units**; Rust does **not** reconvert

- Canonical wire contract: `TronTransaction.energy_limit` and
  `ExecutionContext.energy_limit` carry **energy units**, not SUN.
- Java is the canonical computer of the unit conversion, using its full
  resource/account/fee context (balance, frozen energy, `energyFee`, etc.).
- Rust treats the incoming value as already-in-energy-units: no divide,
  no multiply.

Why this direction:
- Java already has `AccountStore`, `DynamicPropertiesStore`, and fee
  policy in hand; duplicating `getTotalEnergyLimitWithFixRatio` in Rust
  would mean re-reading the same stores and cloning the same policy
  logic — exactly the kind of split authority Phase 1 is closing, not
  widening.
- The Rust divide today is lossy integer truncation against a rate that
  Rust does not own, and silently produces valid-looking-but-wrong
  energy limits. Removing it eliminates a whole class of silent-under-gas
  failures.
- This matches the broader Phase-1 pattern: Java owns tx policy
  (fees, limits, resource accounting inputs); Rust owns execution and
  writes.

Why not the alternatives:
- *Option A (Java sends SUN, Rust divides)*: Requires Rust to reimplement
  Java's `getTotalEnergyLimitWithFixRatio` including
  `origin_energy_limit` and `consume_user_resource_percent` splits for
  `TriggerSmartContract`. Duplicates authority across the boundary and
  fights the Phase 1 ownership model.
- *Option C (add a unit flag/field)*: Legitimizes the mismatch as a
  permanent wire quirk. Phase 1 is about closing ambiguity, not
  codifying it. Rejected.

### 14.5 Migration impact

- **Java bridge (`RemoteExecutionSPI`)**:
  - Fallback branches in `computeEnergyLimitWithFixRatio` (null
    stores, missing account, exception) must stop returning raw
    `feeLimit` and must fail fast instead, raising a clear error that
    this tx cannot be dispatched to remote. Substituting a constant
    divide (e.g. `feeLimit / Constant.SUN_PER_ENERGY`) is **not**
    acceptable: it silently drops the available-energy cap
    (frozen-energy + balance-derived) and ignores dynamic `energyFee`,
    so it produces a wrong-but-plausible energy limit instead of
    surfacing the misconfiguration. Fail-fast is the only Phase 1
    policy, matching the CLAUDE.md "match Java's exact failure
    semantics (strict errors, not defensive recovery)" rule.
  - Default branch at line ~393 that applies to system contracts:
    non-VM contracts do not consume EVM gas, so shipping `feeLimit`
    on those is fine *as long as* Rust does not use it for gas
    metering. Needs a comment clarifying that `energy_limit` is
    semantically undefined for non-VM txs in Phase 1, not
    "happens to be SUN".
  - `TriggerSmartContract` still uses the caller-side formula
    (`computeEnergyLimitWithFixRatio`) instead of the full
    `getTotalEnergyLimitWithFixRatio` that VMActuator uses. That gap
    is acknowledged and tracked as a Phase-1 implementation item,
    not a freeze-level question.
- **Rust execution (`execution/src/lib.rs`)**:
  - Remove the `adjusted_tx.gas_limit = adjusted_tx.gas_limit /
    energy_fee_rate` divide.
  - Update the comment block to reference §14 and to declare that the
    received value is already in energy units.
  - No retry/alternate-unit logic. If the incoming value is out of
    range (0, negative, exceeds `MAX_ENERGY_LIMIT`) the handler must
    reject, not normalize.
- **Fixtures**:
  - Fixture generators must be updated to pre-compute energy units
    before writing `TronTransaction.energy_limit`. Old fixtures that
    wrote `feeLimit` (SUN) are incompatible with the new Rust path
    and must be regenerated. Tracked as a Phase 1 implementation
    item (see todo §1.2 follow-up).
- **`EE-vs-RR` comparison tooling**:
  - No wire changes to the comparison layer, but the under-gas gap
    closes once the Rust divide is removed. Existing Phase-1 parity
    runs that were passing only because both sides equally under-gas
    (unlikely, but theoretically possible for 0-gas trivial txs) will
    need to be re-baselined.
- **Replay tooling**:
  - Replays that captured `tx.energy_limit` directly off the wire
    will store SUN today and energy units after the cutover. Replay
    readers must either (a) re-derive energy units from the captured
    `feeLimit` at replay time or (b) be tagged with a
    wire-contract-version marker so old captures are not fed into
    the new Rust path. Phase 1 picks (b): mark old captures
    explicitly, refuse to replay mixed.

### 14.6 Follow-up implementation items (tracked separately, not in this
    freeze)

- Remove the Rust `gas_limit / energy_fee_rate` divide in
  `execute_vm_transaction`.
- Tighten `computeEnergyLimitWithFixRatio` fallbacks to fail-fast.
- Update conformance fixture generator to emit energy units.
- Add a wire-contract-version marker to replay captures.
- Consider promoting the trigger-side energy computation in Java from
  `computeEnergyLimitWithFixRatio` to a full equivalent of
  `VMActuator.getTotalEnergyLimitWithFixRatio` to close the
  origin/consumer split gap.

### 14.7 Anti-regression note

Once the Rust divide is removed, any new path that wants to send SUN on
`TronTransaction.energy_limit` must either (a) divide in Java first or
(b) extend the proto with an explicit unit marker *and* update this
freeze. Silent reintroduction of the double-divide is the failure mode
this lock is designed to prevent.

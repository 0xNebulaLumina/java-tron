# Close Loop — 5.2 Resource / Fee / Sidecar Parity

This file closes Section 5.2 of `close_loop.todo.md`. Its purpose is to
enumerate every "sidecar" the Rust execution service emits alongside a
transaction's raw state changes, identify which ones are fragile or
incomplete, and make sidecar completeness a first-class input into
contract-family readiness in `close_loop.contract_matrix.md`.

Companion notes:

- `close_loop.bridge_debt.md` — classifies the bridges sidecars travel over.
- `close_loop.contract_matrix.md` — per-contract readiness tags the
  conclusions here feed into.
- `close_loop.write_ownership.md` — `rust_persist_enabled` policy.
- `close_loop.energy_limit.md` — `energy_limit` wire contract.
- `close_loop.storage_transactions.md` — storage transaction semantics.

## Definitions

A **sidecar** in this planning is any field on
`ExecutionResult` (`framework/src/main/proto/backend.proto`) other than
the raw `logs` / `energy_used` / `energy_refunded` / `return_data` /
`error_message` / `status` core. `state_changes` is included as S1
because it is the primary apply channel even though it is not
"side" of anything — it is part of the same audit so the readiness
gates for an `AccountChange`-only contract still flow through this
file. Flat-int counters like `bandwidth_used` (S10) and the unused
`resource_usage` (S11) are also catalogued so the audit covers every
field on `ExecutionResult`. Sidecars exist because:

1. Java still owns some state-mutation paths (maintenance, rewards,
   bandwidth processor) that Rust has not absorbed. Sidecars carry
   the Rust-computed deltas back to Java so Java can apply them to
   its local `AccountStore` / `DynamicPropertiesStore` / etc.
2. Some invariants (e.g. `Account.allowance` + `latestWithdrawTime`
   on WithdrawBalance) live on Java capsules and cannot currently be
   expressed as a straight `StateChange` / `AccountChange`.
3. Some operations (e.g. TRC-10 AssetIssue) require Java-side store
   creation or index updates that `AccountChange` does not cover.

Every sidecar is a symptom of split ownership. The long-term goal in
Phase 2+ is to reduce the sidecar surface to zero by moving the
affected state fully into Rust. Phase 1's job is to stop the existing
sidecars from silently drifting.

## Sidecar inventory

The fields below are all defined on
`backend.proto` `message ExecutionResult` and applied by Java inside
`RuntimeSpiImpl.apply*` (when `rust_persist_enabled = false`) or
mirrored via `postExecMirror` / `ResourceSyncService` (when
`rust_persist_enabled = true`).

### S1. `state_changes` — `StorageChange` + `AccountChange`

Proto: `repeated StateChange state_changes = 5;`
Java applier: `RuntimeSpiImpl.applyStateChangesToLocalDatabase`
(line ~177).

Payload:

- `StorageChange { address, key, old_value, new_value }` for VM storage
  slot writes.
- `AccountChange { address, old_account, new_account, is_creation,
  is_deletion }` where each `AccountInfo` carries `balance`, `nonce`,
  `code_hash`, `code`, plus the optional AEXT fields (`net_usage`,
  `free_net_usage`, `energy_usage`, `latest_consume_time`,
  `latest_consume_free_time`, `latest_consume_time_for_energy`,
  `net_window_size`, `net_window_optimized`, `energy_window_size`,
  `energy_window_optimized`).

Emitted by: every contract type. This is the base state channel.

Java target stores: `AccountStore`; VM storage writes go through the
storage adapter layer.

Completeness assessment: **partial**. Balance and nonce on
`AccountChange` are robust. The other corners are fragile or missing:

- `code_hash` / `code` round-trip is **not yet wired through the
  Java applier**: `RuntimeSpiImpl.applyStateChangesToLocalDatabase`
  has a TODO around line ~1030 for contract code application and
  `updateAccountStorage()` is a no-op around line ~1104. New
  contract bytecode persists today via the `EmbeddedExecutionSPI`
  path, not via `RuntimeSpiImpl.apply*` from a remote result.
- `StorageChange` storage-slot writes are emitted by Rust but the
  Java applier is similarly a no-op for storage rows. VM storage
  effectively round-trips only because `rust_persist_enabled = true`
  is the canonical RR profile and Rust persists directly to its
  own RocksDB; the compute-only profile would silently drop these.
- The AEXT fields rely on the iter 4 B4 pre-exec AEXT snapshot
  bridge (`collectPreExecutionAext`) to round-trip for unchanged
  fields, which is explicitly "we did not port BandwidthProcessor".
  See `close_loop.bridge_debt.md#b4`.

Known gaps:

- Any account field Java's `AccountCapsule` serializes that is not in
  `AccountInfo` silently drops through the gap. Audit against the
  full `Account` protobuf in `core/Tron.proto` — we haven't closed
  whether fields like `account_id`, `assets_issued_name`, or
  `account_resource.storage_limit` round-trip.
- `is_creation` / `is_deletion` flags have not been exercised for
  `SELFDESTRUCT` on the Rust side against the full Java actuator
  deletion logic.
- Contract code and storage application on the Java side
  (`applyStateChangesToLocalDatabase` TODO + no-op
  `updateAccountStorage`) — see above.

Readiness gating: any contract family that mutates non-`AccountInfo`
fields cannot be declared `RR canonical-ready` until its specific
subset of `AccountChange` coverage is verified in a parity test.

### S2. `freeze_changes` — `FreezeLedgerChange`

Proto: `repeated FreezeLedgerChange freeze_changes = 10;`
Java applier: `RuntimeSpiImpl.applyFreezeLedgerChanges` (line ~211).
Gated by Rust config flag `emit_freeze_ledger_changes` (default `true`
in `rust-backend/config.toml`, default `false` in `config.rs` —
see `close_loop.config_convergence.md`).

Payload: `{ owner_address, resource (BANDWIDTH|ENERGY|TRON_POWER),
amount, expiration_ms, v2_model }`.

Emitted by (today, audited against `crates/core/src/service`):

- FreezeBalance (V1)
- UnfreezeBalance (V1)
- FreezeBalanceV2
- UnfreezeBalanceV2

**Not emitted today** (the handler returns
`freeze_changes: vec![]` even though the contract semantically moves
freeze state):

- WithdrawExpireUnfreeze, CancelAllUnfreezeV2, DelegateResource,
  UnDelegateResource. These were originally intended to participate
  in S2 but the Rust handlers never populate the field. An earlier
  draft of this file listed them as "tbd" emitters; that was wrong —
  they are **missing**.

(`WithdrawBalanceContract` is a balance / allowance operation, not a
freeze operation, and is correctly listed under S6/S7 instead.)

Java target stores: directly on `AccountStore` fields (`frozen`,
`frozen_supply`, `unfrozen_v2`, etc.) via `AccountCapsule` setters,
plus `DynamicPropertiesStore` freeze aggregates.

Completeness assessment: **partial**. The field is an "absolute value
after operation" snapshot of the freeze row, not a delta — so it
requires Java to trust Rust's computation of the new total rather
than computing it from an old value. That is fine for V1, but V2 has
a more complex "unfrozen_v2 list with expirations" shape that a
single `amount` + `expiration_ms` does not fully represent.

Known gaps:

- The V2 unfrozen list is a repeated sub-message on `AccountCapsule`,
  not a single row. The current sidecar shape can only describe one
  row at a time; multi-row operations (e.g. `CancelAllUnfreezeV2`
  which touches every pending unfreeze entry on the account) are
  either dropped to the legacy `apply*` path or forced to emit many
  `FreezeLedgerChange` rows per transaction.
- `TRON_POWER` resource emission is defined but not yet exercised
  end-to-end in the parity tests.
- The `v2_model` boolean collapses "V1 vs V2" into one bit; it does
  not distinguish "V1 only", "V2 only", or "mixed V1+V2 account"
  cases that Java's actuators handle separately.

Readiness gating: no freeze/unfreeze contract family can be declared
`RR canonical-ready` until these gaps are covered by targeted parity
tests. All freeze-family contracts are `RR candidate` in
`close_loop.contract_matrix.md`; none have cleared this bar yet.

### S3. `global_resource_changes` — `GlobalResourceTotalsChange`

Proto: `repeated GlobalResourceTotalsChange global_resource_changes = 11;`
Java applier: `RuntimeSpiImpl.applyGlobalResourceChange`
(line ~432, called from inside `applyFreezeLedgerChanges`).
Gated by Rust config flag `emit_global_resource_changes` (default
`true` in `config.toml`, `false` in `config.rs`).

Payload: `{ total_net_weight, total_net_limit, total_energy_weight,
total_energy_limit }`.

Emitted by (today): FreezeBalance, UnfreezeBalance, FreezeBalanceV2,
UnfreezeBalanceV2.

Not emitted, by design (Java actuator does NOT update
`DynamicPropertiesStore` totals on these paths either, so there is
nothing for Rust to mirror): DelegateResource, UnDelegateResource,
WithdrawExpireUnfreeze. An earlier draft of this file marked these
as "missing"; that overcorrected — they are **n/a**.

**Missing today**: CancelAllUnfreezeV2 — Java's
`CancelAllUnfreezeV2Actuator` does mutate the freeze totals
(re-freezing unexpired entries pulls weight back into the totals),
but the Rust handler returns `global_resource_changes: vec![]`.
This is a real gap.

`GlobalResourceTotalsChange` also has a structural omission: it
only carries `total_net_weight` / `total_net_limit` /
`total_energy_weight` / `total_energy_limit`. It does NOT carry
`TOTAL_TRON_POWER_WEIGHT`, which Java's freeze/cancel-all paths
mutate alongside the net/energy totals. Adding this would be a
proto schema change, tracked as a follow-up rather than fixed in
Phase 1.

Java target stores: `DynamicPropertiesStore.totalNetWeight` /
`totalNetLimit` / `totalEnergyWeight` / `totalEnergyLimit`.

Completeness assessment: **robust for the freeze/unfreeze path**,
**missing on the cancel-all path**, **n/a for delegation and
withdraw-expire-unfreeze** (Java does not mutate the totals there
either). The freeze-path emission exists specifically to fix the
"FREE_NET vs ACCOUNT_NET divergence" in which Java's
`BandwidthProcessor` computed `netLimit=0` because `totalNetWeight`
had not been updated yet for freeze operations earlier in the same
block.

Known gaps:

- `CancelAllUnfreezeV2Contract` does mutate freeze totals (re-freeze
  on unexpired rows) and the Rust handler does NOT emit any
  `GlobalResourceTotalsChange`. Tracked as `missing`.
- The proto omits `total_tron_power_weight`. Java's
  `DynamicPropertiesStore` mutates that key on freeze and cancel-all
  paths but the sidecar never carries it, so any caller that
  watches Java's TRON_POWER total sees stale data after a remote
  freeze. Tracked as a follow-up structural fix; not closed in
  Phase 1.
- The field is "absolute totals after operation", same idempotency
  pattern as S2. Two sidecars in the same transaction with different
  totals would indicate a Rust-side bug, but nothing in Java
  enforces that.

Readiness gating: only the freeze/unfreeze family depends on this
sidecar being present AND complete. Delegation, withdraw-expire,
and cancel-all paths either don't need it (delegation /
withdraw-expire are `n/a`) or have their own missing-row entry
(cancel-all). See S2 readiness note.

### S4. `trc10_changes` — `Trc10Change`

Proto: `repeated Trc10Change trc10_changes = 12;`
Java applier: `RuntimeSpiImpl.applyTrc10Changes` (line ~470).
Gated by Rust config flag `trc10_enabled` (plus per-contract flags
for ParticipateAssetIssue / UnfreezeAsset / UpdateAsset).

Payload: oneof `{ asset_issued, asset_transferred }`. Each branch
carries a substantial struct with owner, amounts, token id, and
either the full asset-issue metadata (name/abbr/supply/rates/etc.)
or the sender/recipient/amount triple.

Emitted by: AssetIssue (`asset_issued`), TransferAsset
(`asset_transferred`), CreateSmartContract with non-zero
`call_token_value` (`asset_transferred`).

Java target stores: `AccountStore` (asset maps), `AssetIssueStore` /
`AssetIssueV2Store`, plus the asset-issue indices managed by Java.

Completeness assessment: **partial**. The proto `oneof` has only two
variants (`asset_issued`, `asset_transferred`). The proto itself
explicitly calls out that `Trc10Participated` and `Trc10Updated`
variants are "future" work. That means:

Known gaps:

- `ParticipateAssetIssueContract` has a `participate_asset_issue_enabled`
  feature flag but no corresponding `Trc10Participated` sidecar; any
  side effects of participation (sender balance decrement + asset
  delivery + asset-issuer balance increment) have to flow through
  `AccountChange` + separate `Trc10AssetTransferred` messages. The
  sum must match Java's `ParticipateAssetIssueActuator` output exactly;
  no parity check exists yet.
- `UpdateAssetContract` has `update_asset_enabled` but no
  `Trc10Updated` sidecar, so metadata updates (url, description,
  limits) must travel through `StateChange` rows against the
  asset-issue storage rows directly. Whether that actually works
  round-trip is **TBD**.
- `UnfreezeAssetContract` has `unfreeze_asset_enabled` but there is
  no dedicated sidecar; balance delta goes through `AccountChange`
  and asset frozen_supply goes through storage rows. Again, parity
  is **TBD**.

Readiness gating: no TRC-10 family contract can be declared
`RR canonical-ready` until Participate/Update/UnfreezeAsset have
parity tests OR the missing `Trc10Change` variants are added.

### S5. `vote_changes` — `VoteChange`

Proto: `repeated VoteChange vote_changes = 13;`
Java applier: `RuntimeSpiImpl.applyVoteChanges` (line ~519).

Payload: `{ owner_address, repeated Vote votes }` where `Vote =
{ vote_address, vote_count }`. The list replaces `Account.votes`
wholesale — it is not a diff.

Emitted by: VoteWitness.

Java target stores: `AccountStore` (`Account.votes` field) AND
`VotesStore` (`VotesCapsule` with old_votes and new_votes).

Completeness assessment: **robust for the common case**, but the
two-store update pattern was itself a parity fix (see the CLAUDE.md
lesson on "VoteWitness Dual Store Pattern" — the original
implementation only updated `accountStore.put` without
`votesStore.put`). Any future contract that mutates votes indirectly
(e.g. a future "clear all votes on withdraw" operation) would need
the same dual-store treatment.

Known gaps:

- `old_votes` seeding on first `VotesRecord` creation is gated by
  the `vote_witness_seed_old_from_account` config flag. If the flag
  is false, the maintenance cycle's vote delta computation gives
  wrong results. The flag defaults to `true` but the test suite
  does not actively cover the `false` path.
- `VoteChange` does not carry a timestamp or cycle number, so it
  relies on the ambient `block_timestamp` from the execution context.
  Delayed application (e.g. via `ResourceSyncService` post-block
  sync) could in principle see a different cycle boundary than the
  execution context assumed.

Readiness gating: `VoteWitnessContract` is `RR candidate` and depends
on this sidecar. Moving it to `canonical-ready` requires exercising
the full maintenance-cycle vote delta computation in EE-vs-RR replay,
not just a single-vote round-trip.

### S6. `withdraw_changes` — `WithdrawChange`

Proto: `repeated WithdrawChange withdraw_changes = 14;`
Java applier: `RuntimeSpiImpl.applyWithdrawChanges` (line ~609).

Payload: `{ owner_address, amount, latest_withdraw_time }`. The
`amount` is the withdrawn amount (equal to `Account.allowance`
before the operation); `latest_withdraw_time` is the block timestamp
to set as `Account.latestWithdrawTime`.

Emitted by: WithdrawBalance.

Java target stores: `AccountStore` fields `allowance` (reset to 0)
and `latestWithdrawTime`.

Completeness assessment: **narrow but correct** for Phase 1
WithdrawBalance. The balance delta itself is still handled via
`AccountChange` — this sidecar only handles the two allowance /
time fields that are awkward to express as a balance mutation.

Known gaps:

- The Phase 1 `WithdrawBalance` implementation uses `Account.allowance`
  only — it skips delegation / mortgage reward computation. That
  gap is tracked in `close_loop.contract_matrix.md` for
  `WithdrawBalanceContract` and is NOT a sidecar gap — the sidecar
  itself correctly captures what the narrow path computes.
- `WithdrawExpireUnfreeze` uses the `tron_transaction_result` receipt
  passthrough (S7) for its `withdraw_expire_amount` field instead of
  a dedicated `WithdrawChange` row. Nothing ensures the two stay
  consistent.

Readiness gating: `WithdrawBalanceContract` stays `RR candidate`
until the full delegation/mortgage path is implemented; this is
tracked against the contract matrix, not against this sidecar.

### S7. `tron_transaction_result` — receipt passthrough

Proto: `bytes tron_transaction_result = 15;`
Java applier: read inside `ExecutionProgramResult.fromExecutionResult`;
the serialized `Protocol.Transaction.Result` is deserialized into
a `TransactionResultCapsule` and set on `ProgramResult.ret`.

Payload: serialized `Protocol.Transaction.Result` protobuf bytes.
Emitting handlers (audited against `crates/core/src/service/mod.rs`
plus `contracts/{freeze,withdraw}.rs`):

- `withdraw_amount` — `WithdrawBalanceContract` (`contracts/withdraw.rs`).
- `withdraw_expire_amount` — `WithdrawExpireUnfreezeContract` (`mod.rs:6824`
  AND `contracts/freeze.rs:2632`).
- `unfreeze_amount` — `UnfreezeBalanceContract` (`contracts/freeze.rs:1556`).
- `cancel_unfreezeV2_amount` — `CancelAllUnfreezeV2Contract`
  (`mod.rs:7065`).
- `fee` (and other fee receipt fields) — `AccountCreateContract`
  (`mod.rs:3418`).
- `fee` and permission update payload — `AccountPermissionUpdateContract`
  (`mod.rs:4639`).
- TRC-10 asset issuance receipt fields (asset id, etc.) —
  `AssetIssueContract` (`mod.rs:5758`).
- `exchange_id`, `exchange_received_amount`,
  `exchange_inject_another_amount`,
  `exchange_withdraw_another_amount` — Exchange family
  (`mod.rs:10289` / `10618` / `10904` / `11189`).
- `orderId`, `orderDetails[]` — `MarketSellAssetContract`
  (`mod.rs:12089`). MarketCancelOrder does NOT populate this field.

Completeness assessment: **partial coverage, no structural safety**.
The field is an opaque bytes blob — nothing at the proto level
enforces which fields are populated for which contract type. Any
contract that forgets to set its receipt field silently produces a
default-zeroed receipt, which may or may not cause a visible Java-side
failure depending on how Java's `TransactionResultCapsule` constructor
handles missing fields.

Known gaps:

- `Exchange` family field population has not been verified in parity
  tests.
- `MarketSellAsset` `orderId` + `orderDetails[]` is complex (variable
  length) and the Rust-side serialization pattern has not been
  audited against Java's `OrderCapsule` shape.
- `unfreeze_amount` on `UnfreezeBalanceContract` vs the balance delta
  from `AccountChange` is redundant; nothing enforces the two match.

Readiness gating: Exchange family, Market family, and
`CancelAllUnfreezeV2Contract` cannot move out of `RR candidate`
without a receipt-passthrough parity check.

### S8. `contract_address` — new contract address for CreateSmartContract

Proto: `bytes contract_address = 16;`
Java applier: set on `ProgramResult.contractAddress`.

Payload: 20-byte EVM address of the newly created contract.

Emitted by: `CreateSmartContract`.

Java target: read-side only (displayed in RPC responses, stored as
part of the transaction receipt for lookup).

Completeness assessment: **robust**. The field is narrow and has
existed since the iter-0 baseline `create_smart_contract` test that
is still passing.

Known gaps: none identified.

### S10. `bandwidth_used` — flat int64 bandwidth counter

Proto: `int64 bandwidth_used = 8;` on `ExecutionResult`.
Java consumer: `ExecutionProgramResult.fromExecutionResult` line ~154
sets it on the resulting `ProgramResult`.

Payload: a single `int64` count of bandwidth bytes the Rust handler
believes the transaction consumed.

Emitted by: every non-VM contract handler in
`crates/core/src/service` uses a shared helper
`Self::calculate_bandwidth_usage(transaction)` (defined in
`contracts/freeze.rs` around line 2900) that prefers the
Java-supplied `transaction_bytes_size` field when present. VM
execution paths (CreateSmartContract / TriggerSmartContract) and a
handful of stub branches still hardcode `bandwidth_used: 0`.

Completeness assessment: **partial**. The structural problem from
earlier drafts ("many handlers hardcode 0") is mostly addressed by
the shared helper; the remaining real concerns are:

Known gaps:

- The shared helper trusts the Java-supplied
  `transaction_bytes_size`. If Java ever fails to populate that
  field, the helper falls back to a heuristic that may not match
  Java's `BandwidthProcessor.consume` arithmetic exactly.
- VM-path handlers (Create / Trigger) still emit `bandwidth_used: 0`
  in some branches because VM bandwidth is computed by the
  `BandwidthProcessor` on the Java side around the call, not by the
  Rust execution itself.
- No comprehensive parity check exists between Java's
  `BandwidthProcessor.consume` output and Rust's `bandwidth_used`
  emission for any contract type. The non-VM helper is *believed* to
  be correct because the EE-vs-RR canonical-test contracts pass —
  but "passes one VM contract test" is not the same as "matches
  Java for every non-VM contract".

Readiness gating: any contract whose Java embedded path reports a
non-zero bandwidth charge cannot be moved to `RR canonical-ready`
until its Rust handler is verified to emit the same count.

### S11. `resource_usage` — `repeated TronResourceUsage` (dead schema)

Proto: `repeated TronResourceUsage resource_usage = 9;` on
`ExecutionResult`. Each entry: `{ type (TronResource.Type), used,
total, token_id }`.

Java consumer: **none**. The Java SPI's
`ExecutionSPI.ExecutionResult` class
(`framework/.../execution/spi/ExecutionSPI.java`) does not have a
`resource_usage` field at all. Whatever the proto carries is
silently dropped during conversion at the `RemoteExecutionSPI`
boundary; nothing in the Java codebase ever reads it.

Producer side: the Rust grpc converter
(`crates/core/src/service/grpc/conversion.rs` line ~673) hardcodes
`resource_usage: vec![]` with the comment `// Not implemented yet`.

Completeness assessment: **dead schema**. The field is on the wire,
but it has no producer (Rust always sends empty) and no consumer
(Java SPI does not even define the field). It is in the proto for
historical reasons — neither side can meaningfully use it without
both ends being upgraded together.

Readiness gating: zero — nothing depends on this today. Listed in
the audit specifically so a future task does not assume the field
is "implemented but undocumented" and start writing code against
it. If we eventually need this field, the right Phase 2+ move is
either to remove it from the proto AND the Rust struct (admitting
it's unused) or to add it to the Java SPI ExecutionResult class
AND populate it from the Rust side at the same time.

### S9. AEXT pre-execution handshake (not a sidecar, but in scope)

File: `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
`collectPreExecutionAext` at line ~1700.

This is the INVERSE direction — Java → Rust rather than Rust → Java —
but it is structurally the same kind of bridge as the sidecars above
and participates in the same completeness story. Tracked in
`close_loop.bridge_debt.md#b4` (bridge B4).

Readiness impact: every remote transaction carries this handshake by
default (gated by `-Dremote.exec.preexec.aext.enabled=true`, default
`true`). Disabling the flag breaks AEXT-sensitive CSV parity for
every contract family. Covered by this sidecar-parity audit because
it's the other half of the AEXT round-trip that S1's `AccountInfo`
fields depend on.

## Parity checklist (per sidecar × contract family)

Check items below are grouped by sidecar. For each, the question is:
"does this particular (sidecar, contract family) pair have a parity
test that compares EE and RR outputs on a representative transaction,
and does the test currently pass?"

| Sidecar                                       | Family                | Parity test | Status |
| --------------------------------------------- | --------------------- | ----------- | ------ |
| S1 `state_changes` AccountChange              | TransferContract      | yes         | passing |
| S1 `state_changes` AccountChange              | CreateSmartContract   | yes         | passing |
| S1 `state_changes` AccountChange              | UpdateSettingContract | yes         | passing |
| S1 `state_changes` AccountChange (AEXT)       | bandwidth-sensitive contracts | partial | tbd — relies on B4 hybrid mode |
| S1 `state_changes` StorageChange              | VM path               | tbd         | tbd |
| S2 `freeze_changes` V1                        | FreezeBalance / UnfreezeBalance | tbd   | tbd (emitter wired, parity test missing) |
| S2 `freeze_changes` V2                        | FreezeBalanceV2 / UnfreezeBalanceV2 | tbd | **partial** — single-row gap |
| S2 `freeze_changes` delegation                | DelegateResource / UnDelegateResource | no | **missing** — handler returns empty vector |
| S2 `freeze_changes` withdraw-expire           | WithdrawExpireUnfreeze, CancelAllUnfreezeV2 | no | **missing** — handler returns empty vector |
| S2 `freeze_changes` TRON_POWER                | (any)                 | no          | **missing** |
| S3 `global_resource_changes` freeze path      | FreezeBalance family  | tbd         | tbd (emitter wired, parity test missing) |
| S3 `global_resource_changes` delegate path    | DelegateResource family | n/a       | n/a — Java actuator does not mutate totals on this path |
| S3 `global_resource_changes` withdraw-expire  | WithdrawExpireUnfreeze | n/a        | n/a — Java actuator does not mutate totals on this path |
| S3 `global_resource_changes` cancel-all       | CancelAllUnfreezeV2   | no          | **missing** — Java mutates totals, Rust handler returns empty |
| S3 `total_tron_power_weight`                  | freeze + cancel-all paths | no       | **missing** — proto field does not exist |
| S4 `trc10_changes` AssetIssued                | AssetIssue            | tbd         | tbd |
| S4 `trc10_changes` AssetTransferred           | TransferAsset         | tbd         | tbd |
| S4 (missing variant) `Trc10Participated`      | ParticipateAssetIssue | no          | **missing** — proto variant does not exist |
| S4 (missing variant) `Trc10Updated`           | UpdateAsset           | no          | **missing** — proto variant does not exist |
| S4 (missing variant) `Trc10Unfrozen`          | UnfreezeAsset         | no          | **missing** — proto variant does not exist |
| S5 `vote_changes`                             | VoteWitness (single)  | tbd         | tbd |
| S5 `vote_changes` maintenance-cycle delta     | VoteWitness (multi-cycle) | no      | **missing** |
| S6 `withdraw_changes`                         | WithdrawBalance (allowance-only) | tbd | tbd |
| S7 `tron_transaction_result.withdraw_amount`  | WithdrawBalance       | tbd         | tbd |
| S7 `tron_transaction_result.unfreeze_amount`  | UnfreezeBalance       | tbd         | tbd |
| S7 `tron_transaction_result.withdraw_expire_amount` | WithdrawExpireUnfreeze | tbd   | tbd |
| S7 `tron_transaction_result.exchange_*`       | Exchange family       | no          | **missing** |
| S7 `tron_transaction_result.orderId / orderDetails` | MarketSellAsset  | tbd         | tbd — Rust does emit `tron_transaction_result` for SellAsset; parity test missing |
| S7 (no field)                                 | MarketCancelOrder     | n/a         | n/a — Rust handler does not populate `tron_transaction_result` for CancelOrder |
| S7 `tron_transaction_result.fee` (account create) | AccountCreate    | tbd         | tbd — Rust does emit fee receipt; parity test missing |
| S7 `tron_transaction_result.fee` (permission update) | AccountPermissionUpdate | tbd | tbd — Rust does emit fee receipt; parity test missing |
| S7 `tron_transaction_result` (asset issue receipt) | AssetIssue       | tbd         | tbd — Rust does emit; parity test missing |
| S7 `tron_transaction_result.cancel_unfreezeV2_amount` | CancelAllUnfreezeV2 | tbd | tbd — Rust does emit `tron_transaction_result`; parity test missing |
| S8 `contract_address`                         | CreateSmartContract   | yes         | passing |
| S10 `bandwidth_used`                          | (every contract)      | partial     | partial — non-VM handlers go through shared `calculate_bandwidth_usage` helper; VM-path handlers still emit 0; no comprehensive parity check vs Java BandwidthProcessor |
| S11 `resource_usage`                          | (every contract)      | n/a         | n/a — Rust hardcodes empty vector; field is dead schema today |

Legend: **missing** — no parity test and no implementation path; **partial** —
some shape exists but known gaps remain; **tbd** — needs audit against
an actual fixture; **passing** — a canonical test already exists.

## Contract families that cannot declare `RR canonical-ready` until their sidecars are verified

Based on the above, these contract families have sidecar gates that
must clear before they can be moved out of `RR candidate` in
`close_loop.contract_matrix.md`:

- **Freeze / unfreeze family (V1 + V2)** — gated on S2, S3, and (for
  V2) the single-row-per-unfrozen-list gap.
- **Delegation family** — gated on S2 delegation rows only. S3 is
  `n/a` for delegation paths because Java's actuator does not
  mutate freeze totals on delegate / undelegate.
- **WithdrawExpireUnfreeze** — gated on S2 freeze rows for the
  released entries + S7 receipt passthrough. S3 is `n/a` here.
- **CancelAllUnfreezeV2** — gated on S2 freeze rows + S3 emission
  (which IS missing — Java mutates freeze totals on this path,
  Rust handler returns empty) + S7 receipt passthrough.
- **TRC-10 family (Participate / Update / UnfreezeAsset)** — gated
  on the missing S4 proto variants, OR on an explicit decision that
  those operations will travel through `AccountChange` + storage
  rows and an audit proving it works.
- **Exchange family (Create / Inject / Withdraw / Transaction)** —
  gated on S7 receipt parity.
- **MarketSellAsset** — gated on S7 receipt parity including the
  order-list shape.
- **MarketCancelOrder** — does NOT emit S7; gated only on S1
  storage-row parity (because cancel touches market order storage
  rows directly). Listed separately from MarketSellAsset because
  the two have different sidecar surfaces.
- **VoteWitnessContract** — gated on S5 multi-cycle maintenance
  delta parity, not just single-vote round-trip.
- **AccountCreateContract** — gated on S7 fee receipt parity in
  addition to S1. The Rust handler does emit
  `tron_transaction_result` populated with fee accounting; whether
  Java's `TransactionResultCapsule` round-trips that fee field
  identically to the embedded path is **tbd**.
- **AccountPermissionUpdateContract** — gated on S7 receipt parity
  in addition to S1. The Rust handler emits `tron_transaction_result`
  carrying the fee + permission update payload; round-trip parity
  is **tbd**.
- **AssetIssueContract** — gated on S7 receipt parity in addition
  to the S4 `Trc10Change.AssetIssued` channel. The Rust handler
  emits `tron_transaction_result` with asset-issuance receipt fields
  (asset id, etc.); round-trip parity is **tbd**.

Families that do NOT have sidecar gates beyond S1/S8 (and therefore
only need AccountChange + contract_address parity):

- TransferContract
- CreateSmartContract
- UpdateSettingContract
- WitnessCreate / WitnessUpdate
- AccountUpdate

The close_loop Phase 1 whitelist target in
`close_loop.contract_matrix.md` is a subset of this no-sidecar-gate
list, which is not an accident — it's the smallest set we can drive
to parity without first closing any of the Section 5.2 sidecar gaps.

## Decisions (Phase 1 scope)

These are the decisions this audit locks in. None of them require a
proto-breaking change in Phase 1; they are about *what counts as
ready* and *what cannot be declared ready* until later phases.

1. **Sidecar completeness is a first-class input into contract
   readiness.** The `close_loop.contract_matrix.md` "whitelist target"
   cannot include any contract whose sidecar row above is anything
   other than `yes / passing`. Iter 1.5 defined that whitelist as
   TransferContract + CreateSmartContract + UpdateSettingContract —
   those three specifically because they have no sidecar gate
   beyond S1 and S8.

2. **Missing TRC-10 sidecar variants stay missing in Phase 1.**
   `Trc10Participated`, `Trc10Updated`, and `Trc10Unfrozen` are NOT
   added to the proto in Phase 1. The decision instead is that
   Participate / Update / UnfreezeAsset remain `RR candidate` and
   are NOT on the Phase 1 whitelist target. Adding those proto
   variants is Phase 2 work because it touches the Java applier
   API at the same time.

3. **Multi-row freeze changes stay out of scope in Phase 1.** Any
   transaction that would need to emit more than one
   `FreezeLedgerChange` per account per call (e.g.
   `CancelAllUnfreezeV2`) must fall back to the legacy `apply*`
   path for now. The Rust handler should be audited to confirm it
   does not silently drop the extra rows — this is tracked as a
   follow-up, not closed here.

4. **Receipt passthrough `tron_transaction_result` is a flat-file
   contract, not a structured one.** Adding per-field proto safety
   (typed messages per contract family) is Phase 2 work. Phase 1
   enforces correctness only for the three families already in
   active parity work: `WithdrawBalance`, `UnfreezeBalance`,
   `WithdrawExpireUnfreeze`. Exchange / Market / CancelAllUnfreezeV2
   are explicitly **not** guaranteed in Phase 1.

5. **AEXT round-trip stays on the B4 bridge.** Replacing
   `collectPreExecutionAext` with a native Rust `BandwidthProcessor`
   is Phase 2 work; Phase 1 accepts the hybrid handshake as a
   durable workaround and gates sidecar S1 AEXT readiness on B4
   behavior rather than on a Rust-native implementation.

## Follow-up implementation items

These are the concrete next-step items this audit produces. They are
NOT closed in Phase 1; they are listed to make the debt visible.

- [ ] Audit every `tbd` row in the parity checklist above and flip it
      to either `passing` or `missing` based on an actual test run.
- [ ] Add missing parity tests for S2 delegation rows (DelegateResource
      / UnDelegateResource).
- [ ] Add S3 `global_resource_changes` emission on the
      `CancelAllUnfreezeV2Contract` path (Java mutates the totals
      there, Rust handler currently returns empty). Delegation and
      withdraw-expire paths are deliberately n/a per the S3 audit.
- [ ] Add `total_tron_power_weight` to the `GlobalResourceTotalsChange`
      proto schema and start populating it on the freeze /
      unfreeze / cancel-all paths. This is a structural follow-up,
      not a Phase 1 deliverable.
- [ ] Decide whether `Trc10Participated` / `Trc10Updated` /
      `Trc10Unfrozen` proto variants land in Phase 2 or whether the
      affected contracts stay on `apply*` forever.
- [ ] Add receipt-passthrough parity assertions for
      `WithdrawBalance.withdraw_amount` and
      `UnfreezeBalance.unfreeze_amount` in the existing parity tests.
- [ ] Audit whether `CancelAllUnfreezeV2Contract` actually emits all
      required `FreezeLedgerChange` rows, or silently drops some.
- [ ] Add a multi-cycle VoteWitness maintenance-delta parity test so
      S5 can move from "robust for common case" to "robust in general".
- [ ] Update `close_loop.contract_matrix.md` tables as each sidecar
      row above flips, so the matrix and this file stay in sync.

## Anti-pattern guard

**Do not move a contract from `RR candidate` to `RR canonical-ready`
in `close_loop.contract_matrix.md` by hand without checking this
file's parity checklist first.** If any row involving that contract
is `tbd` / `partial` / `missing`, the move is premature regardless
of what individual unit tests report — the sidecar column is an
AND-requirement on top of the contract-level coverage columns.

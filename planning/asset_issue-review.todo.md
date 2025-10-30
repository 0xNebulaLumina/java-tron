# TRC-10 Remote Execution — Review TODOs (Phase 1 completion + Phase 2 prep)

Status: Draft plan (actionable backlog)

Goal: Close gaps between current implementation and planning docs, ensure parity with embedded actuators, and prepare for optional Phase 2 (full persistence in Rust). This plan assumes we finish Phase 1 “emit-and-apply” cleanly, then optionally move to Phase 2 behind flags.

---

## Current State (as of commits 32047c9, b06a76e)

- Java mapping present and gated: `RemoteExecutionSPI` maps `AssetIssueContract` and `ParticipateAssetIssueContract` to NON_VM with `-Dremote.exec.trc10.enabled` gate (fallback to actuators when disabled). TRC-10 transfer remains stubbed.
- Proto extended: `Trc10Op`, `FrozenSupply`, `Trc10LedgerChange`, and `repeated trc10_changes` in `ExecutionResult` are defined.
- Rust backend: handlers for ISSUE/PARTICIPATE parse payloads and emit `Trc10LedgerChange`; dispatch gated by `execution.remote.trc10_enabled`.
- Java apply entry point added: `RuntimeSpiImpl.applyTrc10LedgerChanges(...)` with toggler `-Dremote.exec.apply.trc10`.
- Gaps:
  - Missing Java plumbing to carry `trc10_changes` from gRPC to `ExecutionProgramResult`.
  - Apply logic in `RuntimeSpiImpl` does not yet mirror actuator parity (remainSupply, fee, ALLOW_SAME_TOKEN_NAME behavior, account issued fields, frozenSupply on account, validations for participate, etc.).
  - Tests and observability not yet added.

Decision: Proceed with Phase 1 (emit in Rust, apply in Java) to completion. Track Phase 2 (persist in Rust) as optional follow‑up behind flags.

---

## Phase 1 — Complete Emit-and-Apply Path

### P1.1 Java Result Plumbing (gRPC → SPI → ProgramResult)

- [ ] Add TRC-10 DTOs to `ExecutionSPI` (or reuse protobuf types with conversion):
  - [ ] `ExecutionSPI.Trc10Op { ISSUE, PARTICIPATE, TRANSFER }`
  - [ ] `ExecutionSPI.FrozenSupply { long frozenAmount, long frozenDays }`
  - [ ] `ExecutionSPI.Trc10LedgerChange` with fields: `op, ownerAddress, toAddress, assetId, amount, name, abbr, totalSupply, precision, frozenSupply[], trxNum, num, startTime, endTime, description, url, freeAssetNetLimit, publicFreeAssetNetLimit, feeSun(optional)`
- [ ] Extend `ExecutionSPI.ExecutionResult` to include `List<Trc10LedgerChange> trc10Changes` with a getter.
- [ ] Extend `ExecutionProgramResult`:
  - [ ] Add `List<ExecutionSPI.Trc10LedgerChange> trc10Changes` with getter/setter.
  - [ ] In `fromExecutionResult(...)`, copy `trc10Changes` from SPI result.
  - [ ] In `toExecutionResult(...)`, copy `trc10Changes` back to SPI result.
- [ ] Update `RemoteExecutionSPI.convertExecuteTransactionResponse(...)` to:
  - [ ] Iterate `protoResult.getTrc10ChangesList()` and map to `ExecutionSPI.Trc10LedgerChange` (convert enum and nested `FrozenSupply`).
  - [ ] Set `trc10Changes` on the constructed SPI `ExecutionResult` before wrapping into `ExecutionProgramResult`.
- [ ] Sorting/parity:
  - [ ] If multiple TRC‑10 changes can exist per tx in the future, define deterministic ordering (e.g., by `op` then `owner_address` then `asset_id` bytes) before setting on results to stabilize CSVs.

### P1.2 Runtime Apply — Asset Issue Parity (Actuator-complete)

File: `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`

- [ ] Compute `remainSupply = totalSupply - sum(frozenSupply.frozen_amount)` for crediting owner (not full totalSupply).
- [ ] `ALLOW_SAME_TOKEN_NAME` handling:
  - [ ] When `allowSameTokenName == 0` (legacy):
    - [ ] Set `precision = 0` for V2 record.
    - [ ] Persist to both `AssetIssueStore` (name‑keyed) and `AssetIssueV2Store` (id‑keyed).
    - [ ] Add owner balance in both legacy (name‑keyed) and V2 maps as actuator does.
  - [ ] When `allowSameTokenName != 0` (V2 only):
    - [ ] Persist to `AssetIssueV2Store` only.
- [ ] Token ID assignment:
  - [ ] Increment `TOKEN_ID_NUM`, persist via `DynamicPropertiesStore.saveTokenIdNum()`, set `assetIssueCapsule.setId(String.valueOf(tokenIdNum))` for both stores.
- [ ] Deduct issuance fee:
  - [ ] Read `ASSET_ISSUE_FEE` from `DynamicPropertiesStore`.
  - [ ] Deduct from owner balance.
  - [ ] If `supportBlackHoleOptimization()` true → burn via `dynamicStore.burnTrx(fee)`; else credit blackhole account.
- [ ] Account updates:
  - [ ] `setAssetIssuedName` and `setAssetIssuedID` on owner.
  - [ ] Set `frozen_supply` on account: convert contract’s frozen list to `Protocol.Account.Frozen` with expire time `start_time + frozen_days * FROZEN_PERIOD` and append to account.
- [ ] Bandwidth/energy: no system energy change; bandwidth accounted by remote result; ensure no double-accounting.
- [ ] Error handling/logging: mimic actuator messages when fail to apply, but do not throw (maintain tx flow), log clearly.

### P1.3 Runtime Apply — Participate Parity (Actuator-complete)

- [ ] Resolve asset for exchange:
  - [ ] If `allowSameTokenName == 0`: resolve via name‑keyed store first (`AssetIssueStore`), else via V2.
  - [ ] If V2 only: resolve by ID in V2 store.
- [ ] Validate basics (mirror actuator as much as feasible in apply step):
  - [ ] `trxNum > 0`, `num > 0`, asset exists, issuer matches `to_address`, current block time within `[start_time, end_time)`, owner/issuer accounts exist.
  - [ ] Owner TRX balance sufficient for `amount` (best effort parity; if not, log and skip apply to avoid divergence; actuator would have failed earlier in embedded path).
- [ ] Compute token amount: `exchangeAmount = floor(amount * num / trx_num)`.
- [ ] Ledger deltas:
  - [ ] Debit owner TRX by `amount`; credit issuer TRX by `amount`.
  - [ ] Debit issuer asset by `exchangeAmount`; credit owner asset by `exchangeAmount` (V1/V2 paths per `allowSameTokenName`).
- [ ] Persist account updates and stores.
- [ ] Logging with before/after balances for troubleshooting parity.

### P1.4 RemoteExecutionSPI Mapping/Toggles Hygiene

- [ ] Keep TRC‑10 mapping gated behind `-Dremote.exec.trc10.enabled` (default false). Confirm consistent messages: “... disabled - falling back to Java”.
- [ ] Ensure pre‑exec AEXT snapshots include Participate recipient (present) and consider TransferAssetContract parity when that’s implemented.
- [ ] Ensure `TxKind=NON_VM` is set for TRC‑10 contracts.

### P1.5 Rust Backend Hygiene (Phase 1)

- [ ] Keep handlers minimal; do not persist TRC‑10 ledgers in Rust.
- [ ] Validation messages: keep error strings close to Java actuators (e.g., “TotalSupply must greater than 0!”, etc.) where practical.
- [ ] Deterministic ordering of emitted state changes (already sorted for some flows); maintain for TRC‑10 too if multiple account changes are emitted.
- [ ] Config default: consider `execution.remote.trc10_enabled=false` by default for safer rollout; enable in dev/test configs as needed.

### P1.6 Tests

Java
- [ ] Mapping fallback test: with `-Dremote.exec.trc10.enabled=false`, TRC‑10 contracts use embedded actuators.
- [ ] Apply step tests:
  - [ ] ISSUE: fabricate `ExecutionProgramResult` with one `Trc10LedgerChange(ISSUE)`; assert stores and owner account match actuator behavior (remainSupply, stores, fee, account fields).
  - [ ] PARTICIPATE: fabricate change; assert asset and TRX balances updated for owner/issuer correctly (name‑keyed vs id‑keyed cases).

Rust
- [ ] Handler unit tests producing expected `Trc10LedgerChange` entries for valid payloads; gating tests when disabled.

Integration (manual)
- [ ] Backend up with `trc10_enabled=true`; Java node with `-Dremote.exec.trc10.enabled=true` and `-Dremote.exec.apply.trc10=true`.
- [ ] RPC: CreateAssetIssue2, ParticipateAssetIssue2; verify via `Wallet` queries consistency with embedded execution.

### P1.7 Observability & Robustness

- [ ] Log key fields on Rust handler entry (name/id/amount) and on Java apply (owner, issuer, tokenId, remainSupply/exchangeAmount).
- [ ] Guardrails: `applyTrc10LedgerChanges` catches and logs, does not break transaction flow.
- [ ] Metrics: optionally count `remote.trc10.applied` and `remote.trc10.apply_errors` via existing metrics callback or logs.

### P1.8 Acceptance Criteria

- [ ] With both flags enabled, TRC‑10 Issue and Participate succeed via remote path and stores reflect correct state.
- [ ] Wallet/API parity with embedded path (assets present, balances correct, fee debited correctly).
- [ ] With either flag disabled, TRC‑10 executes via embedded Java actuators unchanged.

---

## Phase 2 (Optional) — Full Persistence in Rust (behind flags)

Note: Only after Phase 1 is stable.

- [ ] Storage adapter extensions in Rust to read/write java‑tron DBs (`asset-issue`, `asset-issue-v2`, `account-asset`, `properties`).
- [ ] Dynamic props helpers: `ALLOW_SAME_TOKEN_NAME`, `ASSET_ISSUE_FEE`, TOKEN_ID_NUM read/increment, min/max frozen limits.
- [ ] Persist ISSUE/PARTICIPATE ledgers in Rust; emit reference id info in result for logging only.
- [ ] Add Java toggle `-Dremote.exec.trc10.apply_in_java` (default true) and disable apply when Rust persistence is on.
- [ ] Migration and safety: run shadow verification comparing Java readbacks vs Rust persistence for a subset of blocks.

---

## Edge Cases & Parity Notes

- `ALLOW_SAME_TOKEN_NAME`:
  - 0: name‑keyed V1 + V2; V2 precision forced to 0; owner credited in both maps; name ‘trx’ allowed? (Actuator forbids name ‘trx’ only when allowSame!=0; mirror that rule.)
  - 1: V2 only; name ‘trx’ forbidden.
- FrozenSupply: respect MAX_FROZEN_SUPPLY_NUMBER and days range in Rust validation; Java apply must compute expire times via `FROZEN_PERIOD` and set account `frozen_supply` list.
- Fees: source from `ASSET_ISSUE_FEE`; burn vs blackhole per `supportBlackHoleOptimization()`.
- Exchange amount: floor division `amount * num / trx_num`.
- Error strings: keep close to actuators for logs, but favor resilience in Java apply (log and continue rather than throw).

---

## Rollout Plan

- Defaults:
  - [ ] Java mapping disabled by default (`-Dremote.exec.trc10.enabled=false`).
  - [ ] Java apply enabled by default (`-Dremote.exec.apply.trc10=true`).
  - [ ] Rust `execution.remote.trc10_enabled=false` by default (enable in dev/test).
- Phased enablement in lower envs; verify parity via API diffs; flip defaults only after confidence is high.
- Easy rollback: disable Java apply and mapping, or disable Rust flag.

---

## Work Breakdown (Checklist)

- Java SPI/Result Plumbing
  - [ ] Add ExecutionSPI TRC‑10 DTOs
  - [ ] Add `trc10Changes` to `ExecutionResult` and `ExecutionProgramResult`
  - [ ] Map from protobuf to SPI in `RemoteExecutionSPI`
- Runtime Apply Parity
  - [ ] Asset Issue: remainSupply, ALLOW_SAME_TOKEN_NAME behavior, fee, account issued fields, frozen list
  - [ ] Participate: asset resolve (V1/V2), exchange calc, TRX and token deltas, validations
- Rust Hygiene
  - [ ] Error string parity and deterministic emission ordering
  - [ ] Config defaults review and startup logs
- Tests
  - [ ] Java mapping fallback test
  - [ ] Java apply ISSUE test
  - [ ] Java apply PARTICIPATE test
  - [ ] Rust handler happy-path and gating tests
- Observability
  - [ ] Logs and metrics for apply path
- Phase 2 (Optional)
  - [ ] Storage adapter + persistence
  - [ ] Java toggle to disable apply when Rust persists

---

## Acceptance Gates Before Merge to Develop

- [ ] All Java unit tests for apply path pass deterministically.
- [ ] Rust unit tests for handlers pass.
- [ ] Manual end‑to‑end with flags ON shows identical Wallet/API results vs embedded for at least: 3 issuances (V1 and V2) and 5 participations with various rates/time windows.
- [ ] Checkstyle and CI green; no regressions in unrelated modules.


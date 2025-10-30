• I’m going to summarize the first mismatch by comparing the two CSVs and correlating the logs from both runs.

First Mismatch

- Tx: WitnessCreateContract
- Block: 3189, index 0
- Tx ID: f0b6b6afc8bb55f4b1d8b4084f33095eabb2a5f6761de21e7609907c672c1a9a
- CSVs: output-directory/execution-csv/20251030-055539-b3bf2f9c-embedded-embedded.csv, output-directory/execution-csv/20251030-094533-e949456c-remote-remote.csv
- What differs: Only state_digest_sha256
    - Embedded digest: fba180575e93887e2aada757cd37c7246f2f2b5514fde16eaffddc2c983a57fb
    - Remote digest:   668db1bd38a5d29edd3d423fd9ecd3a566e3b9b33fa82fc72e446bed48257b8f

What’s Actually Different

- The per‑tx results agree on success, energy_used, and there are exactly 2 account-level changes.
- The difference is in the account snapshot for the “Blackhole” address TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy (hex 4177944d19c052b73ee2286823aa83f8138cb7032f):
- Immediately previous transaction (index 1122) is AssetIssueContract, which credits the Blackhole by exactly 1,024,000,000 SUN (1024 TRX) in both runs. For that tx, both CSVs agree:
    - 1122 Blackhole old: 9223372086955175808 → new: 9223372087979175808 (+1024 TRX)
        - embedded[1123].old == embedded[1122].new (true)
    - Remote old Blackhole equals previous tx’s old Blackhole (stale carry-over):
        - remote[1123].old == remote[1122].old (true)
other words, “old” must be the pre-state immediately before this tx executes.
- Interim mitigation in Java (if you want CSV parity sooner):
    - In remote mode, prefer journaled state changes (via StateChangeRecorderBridge) for CSV instead of the remote-provided old/new pairs. That guarantees “old” reflects the true pre-apply store state.
    ExecutionCsvRecordBuilder currently uses remote stateChanges for remote mode; switching to journaled changes would align both modes at the CSV layer.
- Would you like me to scan further for the next mismatches to confirm this pattern (i.e., every time an account is modified by consecutive txs in the same block, remote “old” is stale)? I can produce a short
report listing all such occurrences.


----


• Here’s a concrete, Rust‑backend–focused plan to make AccountChange.old_account reflect the true pre‑state right before each transaction executes, including all prior writes in the same block.

Root Cause Recap

- Remote AssetIssueContract in Rust returns only a placeholder AccountChange and Trc10LedgerChange; it does not persist TRX ledger effects (owner debit + blackhole credit). Java synthesizes these into the
CSV, but the Rust backend DB state doesn’t change.
- Next tx (WitnessCreateContract) in the same block reads the blackhole account from the DB and uses that as old_account. Because AssetIssue’s TRX ledger wasn’t written to DB, old_account is stale by 1024
TRX, causing the digest mismatch.
- Fix requires reading “current block overlay” (including prior block-local writes and predicted TRC‑10 ledger effects) for old_account, not just DB-at-block-start.

Design Goals

- Per‑block, per‑address overlay that:
    - Feeds get_account/old_account reads from a “current state” cache including all previous tx writes in the same block.
    - Applies ephemeral TRC‑10 ledger deltas (e.g., AssetIssue fees) to the overlay so subsequent txs see them, without double‑writing to persistent DB.
    - Clears cleanly on block boundary changes (block number/timestamp/witness).
- No Java changes required for MVP. Optional gRPC extension later to pass precise pre‑exec snapshots.

Changes Overview

- Add a BlockExecutionOverlay that sits alongside the storage adapter and is shared across execute_transaction calls for the same block.
- Route all state reads/writes in non‑VM/system/TRC‑10 handlers through this overlay.
- For TRC‑10 contracts (AssetIssue, ParticipateAssetIssue), compute expected TRX ledger deltas and apply them to the overlay only (no DB write), so old_account in subsequent txs is correct.
- Preserve current behavior for DB persistence on non‑TRC‑10 writes (e.g., WitnessCreate: owner debit, optional blackhole credit) and update overlay to match.

Detailed Plan

1. Introduce a block-scoped overlay

- New struct: BlockExecutionOverlay
    - Map: Address -> AccountInfo (latest seen state within the block)
    - APIs:
        - get_account(address) -> Option<AccountInfo>
        - put_account(address, AccountInfo)  // update overlay to match latest write
        - apply_delta(address, i128)         // safe add/sub SUN to balance in overlay
        - reset(block_key)                   // clear when block changes
    - Block key: use TronExecutionContext fields (e.g., block_number + block_timestamp + witness) for uniqueness.
- Ownership and lifecycle:
    - Add an Arc<RwLock<HashMap<BlockKey, BlockExecutionOverlay>>> field to BackendService.
    - In grpc::Backend::execute_transaction, before dispatch:
        - Lookup or create the overlay for the incoming block key.
        - If block key differs from last‑seen, drop the old overlay and construct a fresh one.

2. Wrap account reads to use overlay-before-DB

- Implement helper functions in BackendService:
    - overlay_get_account(overlay, storage_adapter, address) -> AccountInfo
        - If present in overlay, return it.
        - Else read from DB via storage_adapter.get_account(). Cache into overlay and return it.
    - overlay_put_account(overlay, storage_adapter, address, new_account, persist_db: bool)
        - Always update overlay.
        - If persist_db is true, call storage_adapter.set_account() as today.
- Replace direct storage_adapter.get_account/set_account calls in these paths to the overlay helpers:
    - execute_transfer_contract
    - execute_witness_create_contract
    - Freeze/Unfreeze (freeze.rs) where AccountChange is emitted
    - Any other non‑VM/system handlers that emit AccountChange

3. Ensure old_account uses “pre-write” overlay state

- Pattern for AccountChange construction:
    - Load old via overlay_get_account(...) before any mutation.
    - Compute new in memory.
    - Push TronStateChange::AccountChange { old_account: Some(old), new_account: Some(new) }
    - Persist via overlay_put_account(..., persist_db = true) for real ledger writes (e.g., WitnessCreate, Transfer).
    - For creation cases (no prior account), old_account should be None, consistent with current logic.

4. Shadow TRC‑10 ledger deltas into the overlay (no DB write)

- AssetIssueContract (service/mod.rs: execute_asset_issue_contract):
    - Rust currently emits only Trc10LedgerChange and returns success; it does not touch TRX balances.
    - Compute the TRX fee Sun amount for issuance:
        - Preferred: read from dynamic properties if available (e.g., ASSET_ISSUE_FEE). If not present, fall back to 1_024_000_000 SUN (1024 TRX), matching observed CSV and Java’s default (JsonRpcApiUtil).
        - Determine blackhole address (existing ExecutionConfig fees.blackhole_address_base58, or lookup via storage_adapter helper).
    - Apply deltas in overlay only (do not persist to DB):
        - Owner: overlay.apply_delta(owner, -fee_sun)
        - Blackhole: overlay.apply_delta(blackhole, +fee_sun)
    - Do NOT add AccountChange for these deltas in the Rust execution result here; keep current behavior (Trc10LedgerChange only). The goal is just to make subsequent tx old_account reads reflect reality.
- ParticipateAssetIssueContract:
    - Similarly, compute the TRX movement implied by participation (owner pays TRX; issuer receives TRX), using fields already parsed in this handler (amount/TRX).
    - Apply deltas in overlay only (no DB write, no AccountChange in result for TRC‑10 Phase 1).
- This “shadow overlay” ensures tx N+1 sees tx N’s TRX movements even when they are not persisted by Rust (because Java applies them later).

5. Block boundary management

- In grpc::execute_transaction, detect change of block key:
    - If new key != current, drop the old overlay entry, create a fresh overlay for the new block.
    - Avoid unbounded growth by keeping exactly one active overlay (or small LRU if parallel blocks ever occur).
- Optional: add a cleanup task for overlays older than N minutes.

6. Logging and metrics

- Add debug logs when overlay is used:
    - Reads: “overlay hit/miss” with address and block key.
    - Shadow deltas applied for TRC‑10 with contract type, owner, blackhole, amount.
- Metric counters (optional):
    - overlay_reads, overlay_writes, overlay_shadow_applied.

7. Tests

- Unit tests in crates/core/src/service/tests/trc10.rs and a new test file for block overlay:
    - Scenario: same block contains AssetIssue, then WitnessCreate.
        - Set initial blackhole/owner balances in in‑memory engine.
        - Execute WitnessCreate: verify old_account for blackhole reflects +1024 TRX from overlay; AccountChange.new applies 9,999 TRX on top; verify final overlay and DB for WitnessCreate path are
        consistent.
    - Scenario: ParticipateAssetIssue followed by another non‑VM tx touching same accounts; ensure old_account reflects TRX movement from participation.

8. Compatibility and safety

- Do not persist overlay shadow deltas for TRC‑10 to DB to avoid double-apply (Java actuators or later flows will perform the real writes).
- For non‑TRC‑10 system/non‑VM transactions (transfer, witness create…), keep persisting to DB as today AND update overlay so the overlay mirrors DB.
- If dynamic fee lookup for AssetIssue isn’t available, use the static 1_024_000_000 SUN default behind a config/feature flag (e.g., execution.fees.asset_issue_fee_default), so it’s easy to audit/change.

9. Future extension (optional but recommended)

- Add a gRPC field to ExecuteTransactionRequest for pre‑execution account snapshots (Java → Rust) analogous to the existing AEXT hybrid support:
    - Java can pass exact post-resource/pre-exec snapshots for accounts whose balances it has already adjusted (bandwidth, TRC‑10 fees).
    - Rust overlay seeds from these snapshots, guaranteeing perfect old_account parity without guessing fees.
- If implemented, prefer request‑provided snapshots over computed shadow deltas.

Files to touch

- rust-backend/crates/core/src/service/mod.rs
    - Wire overlay into all non‑VM/system/TRC‑10 handlers; replace direct get/set with overlay helpers.
    - In execute_asset_issue_contract / execute_participate_asset_issue_contract, compute and apply shadow TRX deltas to overlay only.
- rust-backend/crates/core/src/service/grpc/mod.rs
    - Add overlay store to BackendService; manage per‑block lifecycle; pass overlay to execution helpers.
- rust-backend/crates/execution/src/storage_adapter/engine.rs
    - No persistence change required; optionally expose helper to get blackhole address and dynamic properties for fees.
- rust-backend/crates/execution/src/storage_adapter/mod.rs
    - Export any new helper types if needed.
- Tests under rust-backend/crates/core/src/service/tests/

Acceptance Criteria

- For a block with AssetIssue followed by WitnessCreate:
    - Remote CSV’s AccountChange.oldValue/newValue for blackhole in the second tx matches embedded (no 1024 TRX discrepancy).
    - state_digest_sha256 for those two txs matches embedded for the same rows.
- Overlay does not cause double‑apply to DB. DB contents after full block commit remain consistent with Java’s source of truth.

This approach addresses the immediate digest mismatch without forcing full TRC‑10 persistence in Rust, and it aligns old_account with the true pre‑tx state by tracking prior block‑local effects through a
scoped overlay.



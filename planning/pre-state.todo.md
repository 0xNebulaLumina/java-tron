# Pre‑State Overlay Parity Plan (Remote old_account must reflect pre‑tx state)

Context: Remote execution returns AccountChange.old_account built from DB snapshots that don’t include prior writes within the same block (and don’t mirror TRC‑10 ledger effects that Java applies). Result: mismatched state digests when consecutive transactions touch the same accounts (e.g., AssetIssue ➜ WitnessCreate blackhole delta of 1024 TRX).

Goal: In the Rust backend, ensure AccountChange.old_account for each transaction is the true pre‑state immediately before that transaction executes, including all prior writes in the same block and shadowed TRC‑10 ledger effects.

Non‑Goals:
- Don’t fully persist TRC‑10 account ledger mutations in Rust yet (Java remains source of truth for those). We only reflect them for old_account via an overlay.
- No API changes required for Java; optional gRPC enhancements can follow later.

---

High‑Level Approach

- Introduce a per‑block, in‑memory BlockExecutionOverlay that:
  - Caches AccountInfo per address with the latest view for the current block.
  - Feeds account reads to build old_account from the overlay first, falling back to DB when absent.
  - Is updated on every account write performed by Rust (persisted DB writes) and also by shadow TRC‑10 deltas (no DB write).
  - Resets when block context changes (new block number/timestamp/witness).

- Route all non‑VM/system handlers (transfer, witness create, freeze/unfreeze, etc.) through overlay helpers for get/set.
- For TRC‑10 AssetIssue/ParticipateAssetIssue: compute the implied TRX deltas (owner debit, blackhole/issuer credit) and apply them ONLY to the overlay so subsequent transactions see correct old_account, without double‑persisting.

---

Detailed TODOs

1) Overlay Infrastructure

- [x] Add BlockExecutionOverlay structure
  - [x] Data: HashMap<Address, AccountInfo>
  - [x] Methods:
    - [x] get_account(addr) -> Option<AccountInfo> (clone)
    - [x] put_account(addr, AccountInfo)
    - [x] apply_delta(addr, i128) -> Result<()> (safe add/sub SUN on balance; create default account if missing)
    - [x] clear()
  - [x] Derive Debug; ensure no accidental Send/Sync issues (store under Arc<RwLock<...>>)

- [x] Block keying and lifecycle
  - [x] Define BlockKey { block_number: u64, block_timestamp: u64, witness: Address (optional) }
  - [x] Equality/hash based on all fields present in TronExecutionContext
  - [x] Place Map<BlockKey, BlockExecutionOverlay> in BackendService (simplified to Option<BlockExecutionOverlay>)
  - [x] Ensure only one active overlay at a time for simplicity; drop others on block changes
  - [x] Add cleanup policy (e.g., on new block, remove previous overlay)

- [x] Logging and flags
  - [x] Add config flag execution.remote.overlay_enabled (default true)
  - [x] Add flag execution.remote.overlay_shadow_trc10 (default true)
  - [x] Add debug logs for overlay hits/misses and shadow delta applications


2) gRPC Integration (BackendService)

- [x] In grpc::Backend::execute_transaction:
  - [x] Compute BlockKey from request.context
  - [x] Get or create the overlay for the BlockKey if overlay.enabled
  - [x] Pass mutable overlay handle to non‑VM/system execution paths (via BackendService methods)
  - [x] On block change, reset overlay (drop previous)

- [x] Add helper wrappers on BackendService:
  - [x] overlay_get_account(overlay, storage_adapter, address) -> AccountInfo
    - Lookup overlay first; if missing, load from DB (storage_adapter.get_account), cache into overlay
    - If DB returns None, use default AccountInfo (balance=0, nonce=0, empty code hash)
  - [x] overlay_put_account(overlay, storage_adapter, address, new_account, persist_db: bool)
    - Always update overlay; persist to DB when persist_db is true (system/non‑VM writes)
  - [x] overlay_apply_delta(overlay, address, delta_sun: i128)
    - Adjust balance in overlay; clamp/validate underflow; don't persist


3) Wire Overlay Into Non‑VM/System Handlers

- [x] Replace direct storage_adapter.get_account/set_account usages with overlay helpers in:
  - [ ] execute_transfer_contract (not implemented yet)
  - [x] execute_witness_create_contract
  - [ ] freeze/unfreeze contract handlers (crates/core/src/service/contracts/freeze.rs) where AccountChange is emitted (not yet wired)
  - [ ] Any other non‑VM/system paths that produce AccountChange (account update, vote witness, etc.)

- [x] AccountChange construction pattern per handler:
  - [x] Load old via overlay_get_account before mutation
  - [x] Compute new AccountInfo in memory
  - [x] Emit TronStateChange::AccountChange { old_account: Some(old), new_account: Some(new) }
  - [x] Persist new to DB via overlay_put_account(..., persist_db=true) for real ledger writes (e.g., WitnessCreate)
  - [x] For creations (no prior), set old_account=None, consistent with existing logic


4) Shadow TRC‑10 Ledger Effects Into Overlay (no DB write)

- [x] AssetIssueContract (execute_asset_issue_contract):
  - [x] Determine asset issue TRX fee (SUN):
    - [x] Use execution.remote.asset_issue_fee_default config (1_024_000_000 SUN)
    - [ ] Add EngineBackedEvmStateStore::get_asset_issue_fee() reading dynamic property (fallback to config) - DEFERRED
  - [x] Resolve blackhole address (use existing get_blackhole_address)
  - [x] overlay_apply_delta(owner, -fee)
  - [x] overlay_apply_delta(blackhole, +fee)
  - [x] Do NOT emit AccountChange for these deltas here (maintain Phase‑1 behavior: Trc10LedgerChange only)

- [x] ParticipateAssetIssueContract (execute_participate_asset_issue_contract):
  - [x] Parse TRX amount from payload (already parsed)
  - [x] Identify issuer/receiver address (per current handler semantics)
  - [x] overlay_apply_delta(payer, -trx_amount)
  - [x] overlay_apply_delta(receiver, +trx_amount)
  - [x] Keep Phase‑1 behavior (Trc10LedgerChange only; no DB writes here)

- [x] Sorting/state digest parity:
  - [x] Maintain existing deterministic sort for state_changes in each handler
  - [x] Shadow overlay only influences old_account read for subsequent txs


5) Engine/Dynamic Properties Enhancements

- [ ] EngineBackedEvmStateStore: add getters for needed dynamic props
  - [ ] get_asset_issue_fee() -> Result<u64> (key: e.g., ASSET_ISSUE_FEE; fallback to config default)
  - [ ] (If missing) get_blackhole_address() method or ensure existing one is present/used consistently

- [ ] Configuration
  - [ ] Add execution.fees.asset_issue_fee_default to config.toml
  - [ ] Document default values and precedence (dynamic property > config > fallback constant)


6) Tests

- [ ] Unit tests for overlay mechanics
  - [ ] overlay_apply_delta arithmetic (positive/negative; underflow guard)
  - [ ] overlay_get/put paths and caching behavior
  - [ ] BlockKey change resets overlay

- [ ] Integration tests for TRC‑10 shadow + subsequent tx
  - [ ] Scenario: AssetIssue ➜ WitnessCreate (same block)
    - [ ] Seed initial owner/blackhole balances
    - [ ] Execute AssetIssue: assert DB unchanged for TRX balances; overlay reflects owner‑fee, blackhole+fee
    - [ ] Execute WitnessCreate: assert old_account(blackhole) equals overlay state; AccountChange.new applies expected debit/credit; DB persists WitnessCreate writes
    - [ ] Validate computed state digests match embedded expectations for both txs

- [ ] Non‑TRC‑10 scenario
  - [ ] Transfer ➜ Transfer (same addresses): ensure second old_account sees first delta

- [ ] Freeze/Unfreeze interactions
  - [ ] Ensure overlay coherence when freeze handlers perform account writes


7) Telemetry & Feature Flags

- [ ] Add counters/timers (optional): overlay_reads, overlay_writes, overlay_shadow_applied
- [ ] Implement execution.overlay.enabled and execution.overlay.shadow_trc10
  - [ ] If disabled, fall back to current behavior (direct DB reads for old_account)


8) Rollout & Safety

- [ ] Default overlay + shadow_trc10 to enabled to fix digest parity by default
- [ ] Guard risky paths with thorough logging at debug level (addresses, amounts)
- [ ] Ensure no double‑apply to DB: shadow deltas must never be persisted by Rust
- [ ] Memory discipline: keep one overlay per active block; clear on block change; bound map size


Acceptance Criteria

- [ ] For a block where AssetIssue is followed by WitnessCreate:
  - [ ] Remote CSV’s AccountChange.old/new for blackhole in the second tx matches embedded (no 1024 TRX discrepancy)
  - [ ] state_digest_sha256 for both txs matches embedded

- [ ] For consecutive non‑VM transactions modifying same accounts within a block, later txs show correct old_account reflecting earlier writes

- [ ] No regression in DB contents post‑block; overlay affects old_account reads only and mirrors real writes


Future Enhancements (Optional)

- [ ] gRPC pre‑execution snapshots: Java provides authoritative post‑resource/pre‑exec account bytes for specific addresses; Rust seeds overlay from these snapshots for perfect parity without computing shadow deltas
- [ ] Extend overlay to storage slots (if/when we emit storage changes that depend on intra‑block writes)
- [ ] Add LRU for overlays if parallel block execution is introduced


References (implementation touchpoints)

- Non‑VM/system handlers: rust-backend/crates/core/src/service/mod.rs
- Freeze handlers: rust-backend/crates/core/src/service/contracts/freeze.rs
- gRPC entry: rust-backend/crates/core/src/service/grpc/mod.rs
- Storage adapter: rust-backend/crates/execution/src/storage_adapter/engine.rs
- Config: rust-backend/config.toml


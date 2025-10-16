think harder.

no need to start implementing/fixing, just read thru and understand the codes in `rust-backend`, and answer me.

what's the difference/relationship among
+ `StorageAdapter`
+ `InMemoryStorageAdapter`
+ `StorageModuleAdapter`
in rust-backend/crates/execution/src/storage_adapter.rs?

and some of them have same-name func, e.g., `get_freeze_record` & `set_freeze_record`, 
is it duplicate, or redudant, or conflicted? or maybe some of them are not really being used?

---

• I read the file and traced how each piece is used. Here’s the relationship and how the “duplicate” methods fit.

Roles

- StorageAdapter
  - Minimal trait the EVM needs (accounts, code, slots). No TRON‑specific helpers.
  - Defined at rust-backend/crates/execution/src/storage_adapter.rs:489.
- InMemoryStorageAdapter
  - Simple in‑memory implementation for tests and local execution. Implements StorageAdapter and adds a small freeze ledger for tests.
  - Defined at rust-backend/crates/execution/src/storage_adapter.rs:514 and implements the trait at rust-backend/crates/execution/src/storage_adapter.rs:604.
- StorageModuleAdapter
  - Real adapter backed by the gRPC storage engine. Implements StorageAdapter for EVM plus many TRON‑specific helpers (witness, votes, dynamic properties, freeze ledger, account names).
  - Defined at rust-backend/crates/execution/src/storage_adapter.rs:649 and implements the trait at rust-backend/crates/execution/src/storage_adapter.rs:1331.

How they work together

- StorageAdapter is the common interface.
- Both InMemoryStorageAdapter and StorageModuleAdapter implement it so they can be wrapped by StorageAdapterDatabase and fed to REVM.
- TRON‑specific flows (witnesses, votes, freeze ledger, properties) are exposed as inherent methods on StorageModuleAdapter (not part of the trait). Core code that needs them uses the concrete type directly (e.g.,
rust-backend/crates/core/src/service.rs:113, rust-backend/crates/core/src/service.rs:1390).

Same‑name methods (get_freeze_record, set_freeze_record, get_tron_power_in_sun)

- They are not duplicates in the Rust sense: they are inherent methods on different types with similar names by design.
  - InMemoryStorageAdapter:
      - get_freeze_record(&self, &Address, u8) -> Result<Option<FreezeRecord>> at rust-backend/crates/execution/src/storage_adapter.rs:543
      - set_freeze_record(&self, &Address, u8, frozen_amount: u64, expiration: i64) at rust-backend/crates/execution/src/storage_adapter.rs:548
      - get_tron_power_in_sun(&self, &Address, bool) -> Result<u64> at rust-backend/crates/execution/src/storage_adapter.rs:558
      - Uses an Arc<RwLock<HashMap<(Address,u8),FreezeRecord>>> in memory only.
      - Used by unit tests, e.g., rust-backend/crates/execution/src/storage_adapter.rs:2328, rust-backend/crates/execution/src/storage_adapter.rs:2356.
  - StorageModuleAdapter:
      - get_freeze_record(&self, &Address, u8) -> Result<Option<FreezeRecord>> at rust-backend/crates/execution/src/storage_adapter.rs:1164
      - set_freeze_record(&self, Address, u8, &FreezeRecord) at rust-backend/crates/execution/src/storage_adapter.rs:1184
      - add_freeze_amount(&self, Address, u8, amount: u64, expiration: i64) at rust-backend/crates/execution/src/storage_adapter.rs:1197
      - get_tron_power_in_sun(&self, &Address, bool) -> Result<u64> at rust-backend/crates/execution/src/storage_adapter.rs:1225
      - Persists to the “freeze-records” DB via StorageEngine with keys 0x41 + address + resource.
      - Used by core service logic: calculating power for voting at rust-backend/crates/core/src/service.rs:936 and updating freeze ledger at rust-backend/crates/core/src/service.rs:1188.

So there’s no conflict: the methods live on different types. They’re intentionally mirrored so both adapters support the same freeze/power operations, but only StorageModuleAdapter persists to the real backend.

What’s unused or redundant?

- remove_freeze_record on StorageModuleAdapter exists for unfreeze but is not yet called by code paths (referenced in docs): rust-backend/crates/execution/src/storage_adapter.rs:1213. It’s planned, not dead.
- The InMemory freeze helpers are used by tests (including crates/core tests), so they’re not redundant (e.g., rust-backend/crates/core/src/tests.rs:433, rust-backend/crates/core/src/tests.rs:536).
- The trait StorageAdapter intentionally does not include freeze/witness/votes/properties to keep the EVM-facing surface minimal; higher-level TRON logic calls the concrete StorageModuleAdapter directly where
needed.

Summary

- StorageAdapter is the minimal trait; InMemoryStorageAdapter and StorageModuleAdapter are two implementations, the latter adding TRON‑specific APIs on top.
- Same‑name methods like get_freeze_record/set_freeze_record are per‑type helpers with consistent semantics; not conflicting, not duplicates at the trait level.
- They are used: tests use the in‑memory ones; core service uses the StorageModuleAdapter ones. Only remove_freeze_record is currently defined but unused pending unfreeze support.


---

think harder.

no need to start implementing/fixing, do you think you could suggest better namings to reflect their relationship?

---


• Here’s a naming pass that better communicates roles and relationships without changing behavior.

Recommended Renames

- Trait
    - StorageAdapter → EvmStateStore (it’s the minimal EVM-facing state interface)
- In-memory impl
    - InMemoryStorageAdapter → InMemoryEvmStateStore (test/memory version of the same interface)
- Persistent impl
    - StorageModuleAdapter → EngineBackedEvmStateStore or PersistentEvmStateStore (clearly engine/db-backed, not a “module” adapter)
- REVM wrapper
    - StorageAdapterDatabase<S> → EvmStateDatabase<S> or RevmEvmDatabase<S> (a Database impl over an EvmStateStore)

Why this helps

- Unifies the family by centering on “EVM state” (trait) + “in-memory/persistent” (impls).
- Removes the confusing “Module” (the type wraps a StorageEngine, not the module itself).
- Avoids “Adapter” in the trait name (the trait is an interface; the impls do the adapting).

TRON helpers (witness/votes/freeze/properties)

- Optional split for clarity (no code change required now, but good future direction):
    - New trait: TronLedgerStore capturing TRON-specific helpers:
        - Witness: get_witness, put_witness, is_witness
        - Votes: get_votes, set_votes
        - Freeze ledger: get_freeze_record, put_freeze_record, add_freeze_amount, remove_freeze_record, get_tron_power_in_sun
        - Dynamic props/account name: existing getters/setters
    - Impl: EngineBackedEvmStateStore implements TronLedgerStore; InMemoryEvmStateStore can implement only the subset it already supports (freeze/power) for tests.

Method name tweaks (small, consistency wins)

- Align verbs by storage semantics:
    - set_freeze_record → put_freeze_record (matches existing put_witness; “put” implies upsert)
    - Keep get_freeze_record, add_freeze_amount, remove_freeze_record as-is (clear and consistent)
    - Optionally, rename get_tron_power_in_sun → compute_tron_power_in_sun (it computes from ledger rather than “gets” a single value)

Minimal-change variant (if you prefer to keep “Adapter”)

- StorageAdapter → keep
- InMemoryStorageAdapter → InMemoryStorageAdapter (keep)
- StorageModuleAdapter → StorageEngineAdapter or PersistentStorageAdapter
- StorageAdapterDatabase<S> → EvmDatabaseAdapter<S>

These schemes make the relationship obvious at call sites:

- “Use EvmStateStore with InMemoryEvmStateStore for tests, and EngineBackedEvmStateStore in production; wrap it in EvmStateDatabase to feed REVM; use TronLedgerStore when you need TRON-specific data.”
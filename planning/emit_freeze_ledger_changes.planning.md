think harder.

no need to start implementing, just give me a detailed plan on:

- Keep freeze_balance_enabled = true but set emit_freeze_ledger_changes = true
- Extend the Rust backend to emit state changes for:
  - Account freeze records
  - Dynamic totals (totalNetWeight/totalNetLimit) updates
- Implement Java-side handling: in RuntimeSpiImpl, apply these freeze/dynamic property changes to local stores (AccountCapsule freeze lists, DynamicPropertiesStore). This requires adding real logic in updateAccountState/updateAccountStorage to apply these changes — currently it only updates balance.
- add temporary logs to include:
  - Account frozen-for-bandwidth (getAllFrozenBalanceForBandwidth)
  - Dynamic totals (getTotalNetWeight, getTotalNetLimit) right in the CREATE_ACCOUNT inputs, to confirm which one is zero on remote.

---


• Here’s a concrete, end‑to‑end plan to keep remote freeze execution enabled and make netLimit consistent by emitting and applying freeze/dynamic state changes.

- Configuration
    - Set in rust-backend/config.toml:
        - execution.remote.freeze_balance_enabled = true
        - execution.remote.emit_freeze_ledger_changes = true (new behavior gate)
    - Keep “burn vs blackhole” fee modes untouched; this work is orthogonal.
- Wire Format and Semantics
    - Reuse existing state change stream (what RemoteExecutionSPI already consumes).
    - Represent new data as StorageChange records (no new RPC types), with clear, parseable keys:
        - Freeze ledger:
            - address: owner’s 21‑byte Tron address (same as AccountChange)
            - key: ASCII “FREEZE:BW” (bandwidth) or “FREEZE:EN” (energy) or “FREEZE:TP” (tron power), as needed
            - value: FreezeRecord serialized as 16 bytes (amount[8] big‑endian + expiration[8] big‑endian)
            - oldValue/newValue: before/after record (or empty for deletion)
        - Dynamic totals:
            - address: sentinel “DYNPROPS” (ASCII, or 21 bytes of zero — choose one; recommend ASCII “DYNPROPS” for readability)
            - key: ASCII “TOTAL_NET_WEIGHT”, “TOTAL_NET_LIMIT”, “TOTAL_ENERGY_WEIGHT”, “TOTAL_ENERGY_LIMIT” (only NET is required for your case)
            - value: 8‑byte big‑endian u64
    - Rationale: this stays within the existing change stream and is simple to detect on the Java side.
- Rust Backend: emitting state changes
    - Where: rust-backend/crates/core/src/service.rs and crates/execution/src/storage_adapter.rs
    - FreezeBalanceContract and Unfreeze:
        - After computing new frozen amount/expiry (and updating the storage engine), push a StorageChange into the result set with:
            - address = owner
            - key = “FREEZE:BW”
            - old/new = previous/current FreezeRecord.serialize()
        - Update dynamic totals (totalNetWeight/totalNetLimit) if they change:
            - Add StorageChange with address = “DYNPROPS”, keys as above, old/new 8‑byte values.
        - Only emit when emit_freeze_ledger_changes = true.
    - Notes:
        - Use the same endianness for totals as DynamicPropertiesStore (big‑endian u64).
        - For account‑scoped freeze, prefer BANDWIDTH first (your netLimit issue), but keep ENERGY and TRON_POWER behind the same gate for completeness.
- Java: applying storage changes
    - Where: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java
    - Update paths:
        - updateAccountState(...) remains for pure AccountChange (balances, etc.).
        - updateAccountStorage(...) must be extended to handle known storage change keys:
            - If address encodes “DYNPROPS” and key is one of:
                - “TOTAL_NET_WEIGHT”: call DynamicPropertiesStore.saveTotalNetWeight(value)
                - “TOTAL_NET_LIMIT”: call DynamicPropertiesStore.saveTotalNetLimit(value)
                - Record dirty keys via ResourceSyncContext.recordDynamicKeyDirty(...) to preserve flushing order/remote sync logs.
            - If key starts with “FREEZE:” and address is a valid account:
                - Decode FreezeRecord (amount, expiration) from newValue.
                - Update AccountCapsule freeze ledger so getAllFrozenBalanceForBandwidth() reflects it:
                    - For bandwidth: update V1 (Frozen) and/or V2 (FreezeV2) as appropriate for your network rules. Minimal path: set V1 primary freeze (setFrozenForBandwidth(amount, expiration)) if the account uses
                    V1; otherwise add/update V2 with type BANDWIDTH.
                - Persist: AccountStore.put(account.createDbKey(), accountCap), and record dirty via ResourceSyncContext.
        - Ordering: Apply storage changes before the next transaction’s BandwidthProcessor.consume runs; current flow applies changes immediately after RemoteExecutionSPI returns, which is correct for subsequent txs
        in the same block.
    - Guardrails:
        - Unknown keys: ignore safely (no exceptions).
        - Empty newValue → delete record (zero out freeze for that resource).
- Temporary diagnostics (to validate parity)
    - Where: chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java
    - Add to CREATE_ACCOUNT logging (already instrumented) two additional fields:
        - accountFrozenBW = accountCapsule.getAllFrozenBalanceForBandwidth()
        - dynTotals = { totalNetWeight, totalNetLimit } (via DynamicPropertiesStore getters)
    - Add to “windowed” log the exact variables contributing to netLimit and newNetUsage, so we can see which side was zero (account frozen vs totals).
    - Keep these at INFO until parity is confirmed; then demote to DEBUG.
- Validation Plan
    - Unit tests (Rust):
        - FreezeBalanceContract execution with emit_freeze_ledger_changes = true emits:
            - StorageChange for FREEZE:BW with correct old/new FreezeRecord (compare bytes)
            - StorageChange for dynamic totals when expected
    - Unit tests (Java):
        - RuntimeSpiImpl.updateAccountStorage applies:
            - FREEZE:BW → AccountCapsule freeze updated (getAllFrozenBalanceForBandwidth() matches)
            - DYNPROPS totals → DynamicPropertiesStore reflects new values
    - Integration replay:
        - Start from a snapshot just before the region where you saw netLimit=0.
        - Run remote mode with both flags:
            - freeze_balance_enabled = true
            - emit_freeze_ledger_changes = true
        - Verify for block 2458 tx ea03…:
            - Embedded and remote BandwidthProcessor logs show the same netLimit (≈8639)
            - Both use “BANDWIDTH” (no 100,000 SUN create-account fee)
            - CSV baselines align (state digest matches)
    - CSV stability:
        - Emitting ledger/dynamic storage changes will add extra rows to state_changes_json for freeze/unfreeze transactions (not your TransferContract rows). That’s expected; for transfer-only txs you’ll still see
        2 changes.
        - If you need to keep CSV shape identical for non‑freeze txs, hide emission when contract != Freeze/Unfreeze (which is already implicit).
- Rollout and Safety
    - Gate emission with the new config flag (emit_freeze_ledger_changes) so you can roll back without code revert.
    - Keep Java application of these changes idempotent and tolerant (ignore unknown keys).
    - Ensure dynamic totals do not regress:
        - If totals are not found in dynamic properties DB, Java currently throws on getters; when applying, use setters that create the keys if absent.
- Open Choices (pick now for implementation)
    - Sentinel address for dynamic properties:
        - ASCII “DYNPROPS” vs 21‑byte zero; recommend “DYNPROPS” for readability in logs/CSV.
    - Freeze ledger encoding:
        - Stick to 16‑byte FreezeRecord (amount+expiration) to match Rust; Java writes into AccountCapsule with the structured API (no need to persist that raw record).
    - V1 vs V2 freeze:
        - If your chain uses both, update both to keep consistency with getAllFrozenBalanceForBandwidth(); otherwise update the active one.
- Expected outcome
    - With freeze ledger and dynamic totals applied locally on the Java node immediately after remote execution of freeze txs, BandwidthProcessor sees consistent netLimit values for subsequent txs (including your
    2458 tx), so both embedded and remote choose “used BANDWIDTH”, eliminating the 100,000 SUN baseline offsets and digest mismatches.

If you want, I can expand this into a task breakdown with specific file edits (paths and function signatures) and the exact logging lines to add, but the above should be sufficient for implementation by the
respective code owners.
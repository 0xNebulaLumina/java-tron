• Here’s a concrete, code-aware plan to compute exact per-account net_limit and energy_limit via the existing resource processors and enrich account_resource_usage in the CSV builder. No implementation, just a
precise path with where/what to change.

Goal

- Populate account_resource_usage_changes_json with accurate net_limit and energy_limit for every account included in that domain, using the same rules java-tron uses at runtime:
    - Net limit: BandwidthProcessor.calculateGlobalNetLimit(AccountCapsule)
        - chainbase/src/main/java/org/tron/core/db/BandwidthProcessor.java:486
    - Energy limit: EnergyProcessor.calculateGlobalEnergyLimit(AccountCapsule)
        - chainbase/src/main/java/org/tron/core/db/EnergyProcessor.java:96

High-Level Design

- Remote mode:
    - Pre-state (old): compute limits using a pre-state snapshot (frozen sums + global totals) captured before apply.
    - Post-state (new): compute limits after apply using processors against live stores.
- Embedded mode:
    - Pre-state (old): reconstruct from DomainChangeJournal freeze/global entries when available; otherwise, fall back to live store values as an approximation (or use StateChangeJournal/Account pre-bytes
    only to select “old” addresses; freeze sums must come from the journal).
    - Post-state (new): compute via processors against live stores.

Builder Enrichment Flow

- ExecutionCsvRecordBuilder enriches account_resource_usage deltas with net_limit/energy_limit:
    - Identify target accounts: those present in AEXT deltas, already produced by DomainCanonicalizer.extractAccountResourceUsage.
        - framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java: extractFromExecutionProgramResult(...) and extractFromEmbeddedExecution(...)
    - For each address:
        - Compute old limits and new limits.
        - Set net_limit:{old,new} and energy_limit:{old,new} on the AccountResourceUsageDelta objects before canonicalization.

Data Dependencies To Compute Limits

- Per-account inputs:
    - All frozen for bandwidth: AccountCapsule.getAllFrozenBalanceForBandwidth()
    - All frozen for energy: AccountCapsule.getAllFrozenBalanceForEnergy()
- Global totals:
    - Net: DynamicPropertiesStore.getTotalNetLimit(), getTotalNetWeight()
    - Energy: DynamicPropertiesStore.getTotalEnergyCurrentLimit(), getTotalEnergyWeight()
- Flags:
    - supportUnfreezeDelay (V1/V2 model switch)
    - allowNewReward (zero-weight guard in net)
- These are already used by the processors; this ensures identical rounding/logic.

Remote Mode Plan

- Extend PreStateSnapshotRegistry
    - Add per-account captures:
        - captureAccountFrozenTotals(address, frozenForBandwidth, frozenForEnergy).
    - Already captures global totals (and we use total_energy_current_limit as “total_energy_limit” in the snapshot).
    - Optionally capture flags used by limit calculus (supportUnfreezeDelay, allowNewReward) if you want perfect temporal fidelity (usually constant across a tx).
- Capture pre-state in RuntimeSpiImpl
    - In capturePreStateSnapshot(...) before apply:
        - Build a set of affected addresses from result.getStateChanges() where key is empty (account state changes).
        - For each, read the AccountCapsule from AccountStore and capture:
            - getAllFrozenBalanceForBandwidth(), getAllFrozenBalanceForEnergy()
        - Already capturing global totals (ensure “energy limit” uses getTotalEnergyCurrentLimit()).
        - File: framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:1240
- Enrichment in ExecutionCsvRecordBuilder (remote)
    - After AEXT deltas computed and after apply has persisted post-state:
        - Post (new) limits:
            - Construct BandwidthProcessor with ChainBaseManager and call calculateGlobalNetLimit(accountCapsule).
            - Construct EnergyProcessor with (dynamicPropertiesStore, accountStore) and call calculateGlobalEnergyLimit(accountCapsule).
            - Read accountCapsule from AccountStore for each address.
        - Pre (old) limits:
            - Prefer using the processor logic but backed by a “snapshot” view:
                - Create a tiny SnapshotDynamicPropertiesStore wrapper that delegates to the live store except:
                    - getTotalNetWeight(), getTotalNetLimit(), getTotalEnergyWeight(), getTotalEnergyCurrentLimit() return the pre-state snapshot values.
                    - supportUnfreezeDelay(), allowNewReward() can delegate (or optionally snapshot if you captured them).
                - Create a minimal AccountCapsule adapter that only overrides:
                    - getAllFrozenBalanceForBandwidth() → pre-state captured value
                    - getAllFrozenBalanceForEnergy() → pre-state captured value
                - Instantiate processors with the snapshot store and call the same calculateGlobal* methods to get the old limits.
            - Alternative (simpler): use the processor V2 helpers compute formula directly (if supportUnfreezeDelay was true), but the wrapper approach guarantees the same paths/logics/rounding.
        - Set old/new net_limit and energy_limit on the AccountResourceUsageDelta for the address.
- Canonicalization
    - Domains remain canonicalized in DomainCanonicalizer.accountAextToJsonAndDigest; the enrichment happens before we pass deltas to canonicalizer, so JSON ends up with the correct net_limit/energy_limit
    old/new fields.

Embedded Mode Plan

- Sources for “old” and “new”
    - New (post): compute via processors on the live stores as in remote.
    - Old (pre):
        - If DomainChangeJournal has freeze changes for the address:
            - Reconstruct per-resource old amounts and aggregate to two sums:
                - Sum oldAmountSun for BANDWIDTH → frozenForBandwidth_old
                - Sum oldAmountSun for ENERGY → frozenForEnergy_old
            - Use journal’s global_resource_changes to reconstruct pre-state totals (if present), else read live totals (assuming unchanged).
            - Use the same “snapshot store + account adapter” technique to call processor calculateGlobal* and compute old limits.
        - If no freeze changes occurred:
            - Assume the account’s frozen sums did not change this tx; read live account frozen sums and compute both old & new from live state (identical), or leave old limits unset if you prefer strictness.
- Locate these in builder
    - Enrichment occurs in ExecutionCsvRecordBuilder.extractFromEmbeddedExecution(...), symmetrical to the remote case.
        - framework/src/main/java/org/tron/core/execution/reporting/ExecutionCsvRecordBuilder.java:400–420 region where AEXT deltas are built.

Edge Cases & Consistency

- Weights zero
    - If totalNetWeight == 0 or totalEnergyWeight == 0 pre or post, limit = 0 (processors already do this).
- V1 vs V2 (supportUnfreezeDelay)
    - Use the flag value effective at the calculation time (pre or post) to pick the same path processors would. The snapshot-store wrapper ensures you pass in the correct totals; flags may be read from live
    store (typically stable intra-tx).
- Rounding/precision
    - Processors use TRX_PRECISION, windowing, and exact rounding helpers. By calling their calculateGlobal* methods you inherit exact behavior.

Where to Put Things

- New helper(s)
    - AccountLimitEnricher (framework/src/main/java/org/tron/core/execution/reporting/AccountLimitEnricher.java)
        - enrich(List<AccountResourceUsageDelta> deltas, TransactionTrace trace, Mode mode):
            - Mode: REMOTE or EMBEDDED (to decide snapshot vs journal source for “old”).
            - Locates ChainBaseManager via trace.getTransactionContext().getStoreFactory().getChainBaseManager().
            - Creates processors and runs old/new computation per address.
        - getOldTotals(...) and getOldFrozen(...) variants:
            - Remote: read from PreStateSnapshotRegistry for address and global totals.
            - Embedded: read from DomainChangeJournalRegistry freeze/global entries, fallback to live totals.
        - buildSnapshotStore(...) and buildSnapshotAccount(...):
            - Wrappers to feed processors old values.
- Update builder to call AccountLimitEnricher before DomainCanonicalizer.accountAextToJsonAndDigest is invoked.

Testing Plan

- Unit tests for AccountLimitEnricher
    - Feed known frozen sums and totals; verify limits equal those from BandwidthProcessor/EnergyProcessor against a real account in a controlled store.
    - Verify both V1 and V2 paths (supportUnfreezeDelay toggled).
- Integration tests remote
    - result.getStateChanges() includes an account; PreStateSnapshotRegistry captures frozen + totals; after apply, enrich; check that deltas include net_limit/energy_limit old/new and match expected.
- Integration tests embedded
    - Create a tx that freezes or unfreezes; DomainChangeJournal contains old/new freeze/global; enrich; verify old/new limits are correct.
- Regression
    - Ensure CSV header unchanged; only account_resource_usage_changes_json contents gain filled limits (no header change).
    - Verify determinism (digest stable).

Performance Considerations

- Per tx: processors are lightweight and computations O(N_accounts_in_AEXT_deltas).
- Snapshot wrappers are simple value providers; they do not touch disk.

Fallbacks & Rollout

- If pre-state data missing (edge embedded cases), emit only new limits and leave old unset (or set old=new); call this out in logs so you can refine later.
- Keep the enrichment behind the existing csv flag: -Dexec.csv.stateChanges.enabled=true.


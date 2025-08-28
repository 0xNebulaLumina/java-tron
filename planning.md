I want to track/compare whether remote-mode execution behavior is consistent with embedde-mode execution.

you could print
+ ExecutionProgramResult
   + isSuccess
   + energyUsed
   + return data
   + error messages (runtimeError)
   + stateChanges
+ TransactionContext
   + blockCap
   + trxCap
   + programResult
+ stateDigest

to a csv
after executing each tx.

other requirement:
+ don't do it in ShadowExecutionSPI
+ no need to consider shadow mode
+ the main focus is (remote execution + remote storage) vs (embedded execution + embedded storage). and by "embedded execution" I mean the orginil native java execution, the "RuntimeImpl()", instead of EmbeddedExecutionSPI inside "RuntimeSpiImpl()"


---

Restating your goal

- You want to verify that “remote execution + remote storage” produces the same observable behavior as “embedded execution + embedded storage” (where “embedded
execution” is the original Java path RuntimeImpl, not the ExecutionSPI wrapper).
- After each transaction executes, dump key execution and context data to CSV, including:
    - ExecutionProgramResult: isSuccess, energyUsed, return data, runtimeError, stateChanges
    - TransactionContext: blockCap, trxCap, programResult
    - stateDigest
- Do not implement this inside ShadowExecutionSPI, and you don’t need to support shadow mode.

Key constraints and challenges

- The embedded Java path (RuntimeImpl) produces a ProgramResult that doesn’t currently carry stateChanges. Remote execution returns stateChanges via gRPC, so
parity of “stateChanges” requires extra plumbing on the embedded path.
- Storage mode is global (set via Args/CommonParameter + StorageSpiFactory); mixing storage modes within a single process and a single block/tx flow is risky.
- Logging large return data or capturing many state changes may be expensive; CSV writing must be safe and non-blocking under mainnet load.
- A reliable stateDigest should be computed deterministically across modes; StateDigestJni exists, but it expects normalized account info you may not have in
embedded without deeper hooks.

Clarifying questions

- CSV scope: Do you want one combined CSV (both modes together) or one CSV per run/mode and a separate comparator tool to diff them?
- Digest algorithm: Is SHA-256 of normalized state changes acceptable, or do you require StateDigestJni/keccak-based digest? If the latter, are we okay to start
with remote-only digest and add embedded parity later?
- Return data: OK to cap/truncate return payload length (e.g., 4 KB) in CSV?
- Sampling: Do you need every tx or is configurable sampling (e.g., every N tx) acceptable for mainnet performance?

Solution options

1. Two-run, CSV + offline comparator (recommended)

- How it works:
    - Add a CSV logger invoked after each tx execution inside the existing flow (e.g., end of Manager.processTransaction), independent of ShadowExecution.
    - Run the node twice over the same blocks: once configured as embedded+embedded (RuntimeImpl + embedded storage), once as remote+remote (RuntimeSpiImpl with
RemoteExecutionSPI + remote storage).
    - Each run writes a CSV with the same schema and a “mode” banner (exec_mode, storage_mode). A small comparator tool then joins by (blockNum, txIndex or txId)
and reports mismatches.
- Pros:
    - Zero interference with state or consensus logic.
    - Clean separation of modes; real-world fidelity for both configurations.
    - Easy to roll out and revert; safe in production.
- Cons:
    - Requires two replays (time + ops).
    - Embedded path initially won’t have stateChanges; stateDigest parity will start as “best effort”.

2. In-process dual-execute (not Shadow) with rollback

- How it works:
    - For each tx, take a snapshot (via RevokingDatabase/SnapshotManager), run “embedded+embedded,” capture metrics, revert; then run “remote+remote,” capture
metrics, and commit only the canonical path.
    - CSV logs both results; compare inline and/or offline.
- Pros:
    - Single-pass validation; real-time mismatches.
- Cons:
    - Complex and risky: touching snapshot/revert and flipping global storage/execution modes per-tx can cause contention, perf regressions, and correctness
hazards.
    - Requires extra guardrails to prevent state drift or accidental double-apply.
    - Not recommended for production.

3. Integration tests with synthetic CSV artifacts

- How it works:
    - Build a JUnit suite that runs curated tx sets in both modes, emits CSV artifacts, and compares them as an assertion.
- Pros:
    - Deterministic; good for CI signal.
- Cons:
    - Limited to synthetic cases and won’t reflect mainnet variability.
    - Still doesn’t solve production CSV needs.

Recommendation

- Adopt Option 1 (two-run CSV + offline comparator).
    - It’s simple, non-invasive, and aligns with “don’t use ShadowExecutionSPI.”
    - Start with a robust per-tx CSV logger (mode-tagged), then add a companion comparator tool.
    - Defer embedded stateChanges/stateDigest parity to a second phase (instrumentation of the Java VM path), keeping early results useful by comparing core
semantics: success, energyUsed, return data, runtimeError.

Proposed implementation plan

Phase 1: CSV logging (minimal-risk, production-safe)

- Hook point
    - Log at the end of Manager.processTransaction after trace.finalization() and trxCap.setResult(trace.getTransactionContext()).
    - This provides access to TransactionContext, ProgramResult (or ExecutionProgramResult), trxCap, blockCap.
- CSV schema (columns)
    - run_id (timestamp UUID), exec_mode (EMBEDDED/REMOTE), storage_mode (EMBEDDED/REMOTE)
    - block_num, block_id, tx_index_in_block, tx_id
    - contract_type, is_constant (boolean)
    - is_success (derived from Program/ExecutionProgramResult), energy_used
    - return_hex (truncated), return_len
    - runtime_error (sanitized)
    - state_change_count (remote: size of list; embedded: 0 initially)
    - state_digest (remote: computed; embedded: empty or placeholder until Phase 2)
    - ts_ms
- State digest v1
    - Remote: compute SHA-256 over a canonical serialization of stateChanges:
    - Tuple per change: hex(address)|hex(key)|hex(oldValue)|hex(newValue)
    - Sort tuples lexicographically and hash the concatenation. Stable across runs.
- Embedded: leave empty for now (or hash of empty list) and document that Phase 2 adds parity.
- Controls/safety
    - System properties env:
    - `-Dexec.csv.enabled=true|false`
    - `-Dexec.csv.dir=...` (default `output-directory/execution-csv/`)
    - `-Dexec.csv.maxReturnBytes=4096`
    - `-Dexec.csv.sampleRate=1` (1 = log every tx)
- Non-blocking writer using a bounded queue + background thread; drop-with-metric on backpressure.
- Sanitize newlines/commas; hex-encode binary fields.
- Mode tagging
    - Exec mode: ExecutionSpiFactory.determineExecutionMode()
    - Storage mode: StorageSpiFactory.determineStorageMode()
- Deliverables
    - ExecutionCsvLogger utility with rolling files (e.g., per day/size).
    - Minimal call site in Manager.processTransaction guarded by flag.
    - README snippet on enabling and schema.

Phase 2: Embedded stateChanges + stateDigest parity (optional, higher effort)

- Goal: Add comparable stateChanges for embedded RuntimeImpl.
- Approach options:
    - VM hook: instrument the Java VM (e.g., Program/SSTORE) to collect storage writes into an in-memory list on ProgramResult. This matches the remote semantics
and is the most accurate.
    - Storage tap: add a thin “observer” around DB batch writes during tx execution to collect keys updated across ContractStore/AccountStore. Higher risk and
harder to scope.
- State digest v2
    - Standardize on the same canonical serialization and hash function as remote (keep current SHA-256 or swap to keccak if you want StateDigestJni parity).
- Deliverables
    - ProgramResult (or ExecutionProgramResult.fromProgramResult) enhanced to include embedded stateChanges.
    - CSV logger updated transparently.

Phase 3: Offline comparator tool

- Functionality
    - Input: two CSVs from separate replays: embedded.csv and remote.csv.
    - Join rows by (block_num, tx_id) (fallback to (block_num, tx_index_in_block)).
    - Compare per-field with tolerances:
    - is_success, energy_used (exact), return_hex (exact), runtime_error (normalized), state_digest (exact if available).
- Output: summary stats + a mismatches.csv with diffs.
- Controls
    - --fields to select comparison fields.
    - --ignore-return-data to speed up or reduce noise.
- Deliverables
    - Simple Java CLI or a small Python script in scripts/ plus usage docs.

CSV content details

- blockCap: block_num, block_id (hex), is_witness_signed (bool)
- trxCap: tx_id (hex), contract_type, tx_index_in_block, fee_limit
- programResult/ExecutionProgramResult:
    - is_success, energy_used, return_hex/len, runtime_error
    - state_change_count
- stateDigest: sha256(serialized_state_changes) for remote; empty/placeholder for embedded v1.

Environment considerations

- Ensure remote backend is up for the remote run; expose -Dexecution.remote.host/port or the existing ExecutionSpiFactory envs.
- Avoid checkstyle friction by following existing style; wire via system props rather than config changes where possible.
- Do not change consensus or commit paths; CSV writing is side-effect-free.

What I need from you to proceed

- Confirm you prefer Option 1 (two-run CSV + offline comparator).
- Confirm digest choice: OK to start with SHA-256 over normalized stateChanges and add StateDigestJni/keccak in Phase 2?
- CSV layout approval and whether to cap/truncate return data.
- Preferred location for CSV files and comparator tool.

---

- Confirm you prefer Option 1 (two-run CSV + offline comparator): yes, confirm.
- Confirm digest choice: OK to start with SHA-256 over normalized stateChanges and add StateDigestJni/keccak in Phase 2: yes, confirm.
- CSV layout approval and whether to cap/truncate return data: approve layout, don't cap/truncate return data
- Preferred location for CSV files and comparator tool: you can decide.

now no need to code, update and restate the plan 

---

Confirmed. Here’s the updated, concrete plan for Option 1 (two-run CSV + offline comparator), with SHA-256 state digest now and StateDigestJni later, and no
truncation of return data.

Scope

- Compare “remote execution + remote storage” vs “embedded execution (RuntimeImpl) + embedded storage”.
- Log per-tx execution details to CSV after execution (no ShadowExecutionSPI).
- Phase 1 focuses on safe CSV logging and reproducible runs; Phase 2 adds embedded stateChanges parity; Phase 3 provides an offline comparator.

Artifacts & Locations

- CSV outputs: output-directory/execution-csv/<run_id>-<execMode>-<storageMode>.csv
- Comparator tool: scripts/execution_csv_compare.py
- Logger classes: framework/src/main/java/org/tron/core/execution/reporting/:
    - ExecutionCsvLogger.java (writer, rotation, background queue)
    - ExecutionCsvRecord.java (row model)
    - StateChangeCanonicalizer.java (normalize + SHA-256 digest)

CSV Schema (per row)

- run_id: unique per process (e.g., timestamp + UUID)
- exec_mode: EMBEDDED|REMOTE (from ExecutionSpiFactory.determineExecutionMode())
- storage_mode: EMBEDDED|REMOTE (from StorageSpiFactory.determineStorageMode())
- block_num, block_id_hex, is_witness_signed, block_timestamp
- tx_index_in_block, tx_id_hex, owner_address_hex
- contract_type, is_constant, fee_limit
- is_success, result_code, energy_used
- return_data_hex, return_data_len
- runtime_error
- state_change_count
- state_changes_json: JSON array of {address, key, oldValue, newValue} as hex (full content; no truncation)
- state_digest_sha256: SHA-256 over canonicalized, sorted stateChanges tuples
- ts_ms: write timestamp
Notes:
- Embedded Phase 1: state_changes_json empty, state_digest_sha256 hash of empty list.
- Remote Phase 1: full stateChanges emitted and hashed.

Phase 1: CSV Logging (Production-Safe)

- Hook: end of Manager.processTransaction after:
    - trace.finalization() and trxCap.setResult(trace.getTransactionContext()).
    - Access: TransactionContext, ProgramResult (or ExecutionProgramResult), trxCap, blockCap.
- Behavior:
    - Build a row from context and result fields.
    - If ProgramResult is an ExecutionProgramResult with stateChanges, include full JSON and digest; otherwise leave empty and compute digest over zero changes.
    - Background writer flushes to CSV; backpressure: drop with metric (counter).
- Controls (system properties):
    - -Dexec.csv.enabled=true|false (default false)
    - -Dexec.csv.dir=output-directory/execution-csv
    - -Dexec.csv.sampleRate=1 (1 = all tx; higher = sample)
    - -Dexec.csv.rotateMb=256 (optional rotation)
- Safety:
    - RFC 4180 quoting, hex-encode binary fields, JSON properly escaped.
    - No truncation of return data per your requirement.
- Run procedure:
    - Embedded run: execution.mode=EMBEDDED, STORAGE_MODE=embedded.
    - Remote run: execution.mode=REMOTE, STORAGE_MODE=remote, set execution.remote.* host/port to Rust backend.
    - Ensure separate data dirs/configs so replays are independent.

Phase 2: Embedded stateChanges + Digest Parity

- Goal: capture the same stateChanges for the embedded RuntimeImpl.
- Approach (preferred): instrument the Java VM (SSTORE path) to record storage writes into ProgramResult (or into an ExecutionProgramResult.fromProgramResult
enrichment step).
- Alternative (riskier): observe DB batch writes during tx execution to infer storage changes.
- Digest: continue SHA-256 canon initially; optionally add keccak/StateDigestJni parity afterward (behind a flag).
- CSV: populated state_changes_json and digest for embedded.

Phase 3: Offline Comparator Tool

- Location: scripts/execution_csv_compare.py
- Input: two CSV files (remote and embedded runs).
- Join key: primary tx_id_hex; fallback (block_num, tx_index_in_block).
- Field comparisons:
    - is_success, result_code, energy_used: exact
    - return_data_hex: exact
    - runtime_error: exact (phase-in normalization if needed)
    - state_digest_sha256: exact; log size and location of first mismatch
- Output:
    - Summary (counts, mismatch rate by field)
    - mismatches.csv with both sides’ values for diffed fields
    - Optional filters: --fields, --ignore-return-data

Operational Guidance

- Separate state/data directories per run to avoid cross-contamination.
- Ensure Rust backend up and configured for remote run.
- Consider log file rotation if CSV grows large.
- Start with sampleRate=1 and turn off sampling unless performance requires it.

Risks & Mitigations

- Large rows due to return_data and state_changes_json:
    - You requested no truncation; we will honor that. Rotation + disk monitoring recommended.
- Embedded stateChanges parity:
    - Delivered in Phase 2 (VM instrumentation). Until then, digest and JSON will be missing on embedded; comparisons rely on core fields + digest only for
remote.
- Performance overhead:
    - Use async writer; measure queue drops and file I/O latency via simple counters.

Timeline Snapshot

- Phase 1: Add logger, hook in Manager, config flags, docs.
- Phase 2: VM instrumentation for embedded stateChanges; enable full parity + digest.
- Phase 3: Comparator script + usage instructions; CI-friendly outputs.

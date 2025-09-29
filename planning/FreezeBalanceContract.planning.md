> Here’s the first concrete divergence and why it happens.

First Mismatch

- Tx: f3661179fd49b4fd9e7b79ba35f26e07f1709896231f137d274a1d6f59da1560
- Block: 2142, tx_index: 0
- Contract: FreezeBalanceContract
- Evidence:
    - Embedded CSV row: output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv:1042
    - Remote CSV row: output-directory/execution-csv/20250929-095109-a9089995-remote-remote.csv:1042
    - Remote Java logs: remote-java.052d861.log:90287, remote-java.052d861.log:90290–90297

What Embedded Did

- State change count: 1
- Address changed: owner 41e552f6487585c2b58bc2c9bb4492bc1f17132cd0
- Old vs new value (first 32 bytes are the balance):
    - Old balance: 0x000…000002faf080 (50,000,000)
    - New balance: 0x000…000002ebae40 (49,000,000)
- Digest: 8f8e4264efd1be302cf9429acc6d158de6827f3b2652cbff7e4a94b41428fb5a
- Source: output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv:1042

What Remote Did

- State change count: 2
- Both changes target address 410000000000000000000000000000000000000000 (Base58: T9yD14Nj9j7xAB4dbGeiX9h8unkKHxuWwb, the 0x41 + 20 zeros “null”/system address)
- Two records for the same empty key “”, one with old==new, another from empty→same value; effectively a duplicate/no-op on the null address
- Digest: a5d3d4ce7371abaf0a4995ab87194dd0a0fa30da4c0e84f9d674d7bbadf54ef7
- Source: output-directory/execution-csv/20250929-095109-a9089995-remote-remote.csv:1042

Corroborating Logs

- Remote Java clearly applies 2 changes to the 0x41…00 address, not the owner:
    - “Remote execution returned 2 state changes and 0 logs” remote-java.052d861.log:90287
    - “Applying 2 state changes … f366…” remote-java.052d861.log:90290
    - “Updating account state … 410000000000000000000000000000000000000000 … balance 0 -> 0” remote-java.052d861.log:90291–90296

Root Cause (Why They Differ)

- The embedded path records the correct owner account update (balance decreases by the freeze amount), yielding one account-level state change and the expected digest.
- The remote path emits two account-level state changes for the “null/system” address (0x41 + 20 zeros), not the owner, and both are effectively no-ops (old==new or empty->same), causing:
    - state_change_count to be 2 instead of 1
    - state_digest_sha256 to differ
- This points to the remote executor misassigning the state change address for FreezeBalanceContract and duplicating a record. In short, remote returns account changes for the wrong address and misses the actual owner account update.

Likely Implementation Gaps

- Java request mapping: FreezeBalanceContract is not explicitly mapped to remote; by current source it should fall back to embedded (framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:342–364 and default fallback
at framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:407). However, the logs show remote executed this tx. That suggests the run used code/config where FreezeBalance was treated as remotely executable or flagged as
VM, sending it down the EVM path.
- Rust handling:
    - No specific FreezeBalance handler exists in core (rust-backend/crates/core/src/service.rs) to update the owner’s account or frozen balances.
    - If the tx was misclassified as VM, the EVM/adapter path may synthesize/detect spurious account changes on 0x41…00 (e.g., defaulted address, “created” entry, or incorrectly constructed account change), instead of the owner.

Next Checks

- Confirm the tx_kind classification sent to Rust for this exact tx:
    - In Java, verify what RemoteExecutionSPI set for this tx (Non-VM vs VM) and if Freeze was mapped to remote in your run.
    - If you can re-run with extra logging: log tx_kind and contract_type in buildExecuteTransactionRequest and in Rust convert_protobuf_transaction (rust-backend/crates/core/src/service.rs:2621–2625).
- Inspect remote response contents for this tx on the wire (if you captured gRPC payloads) to confirm the address bytes in state changes were 0x41…00.
- If classification was VM, confirm why the EVM path would touch 0x41…00 at all; check StorageAdapterDatabase logging around commit to see what address was marked modified and why (rust-backend/crates/execution/src/storage_adapter.rs:980–
1088).
- Scan subsequent mismatches to see if the same pattern repeats for other system/non-VM contracts (UnfreezeBalance, DelegateResource, etc.) to prioritize fixes.

Recommended Fix Direction

- Short-term parity: ensure FreezeBalanceContract falls back to Java (embedded) until fully implemented in Rust. In your Java mapping, keep FreezeBalance in the default/fallback (throw) path so it never invokes remote execution for this
contract type.
- Correctness in Rust (when enabling):
    - Add a proper non-VM handler for FreezeBalanceContract in rust-backend/crates/core/src/service.rs that:
        - Decodes freeze parameters.
        - Emits exactly one AccountChange for the owner address with the new balance.
        - Updates and emits any necessary frozen resource bookkeeping addresses if/where applicable, matching embedded’s journaling conventions.
    - Ensure the emitted address for the account state change is the real owner (21-byte Tron address: 0x41 + 20 EVM bytes) and not 0x41…00.
    - De-duplicate change emission—avoid adding two identical account-level changes for the same key.

Want me to continue with the next mismatched tx and build a small summary to see if this same “0x41…00 duplicate change” pattern recurs?


---


think harder.
no need to start implementing, just give me a detailed plan to implement proper FreezeBalanceContract handling in the Rust backend


---


> Here’s a concrete, end-to-end plan to implement proper FreezeBalanceContract handling in the Rust backend, aimed at CSV/digest parity first, then full semantics.

Goals

- Return correct state changes for FreezeBalanceContract:
    - Exactly 1 AccountChange for the owner with balance decreased by frozen amount.
    - energy_used=0, logs=[], deterministic ordering.
- Avoid spurious changes for 0x41…00 and any duplicates.
- Keep non-VM path and system contract gating consistent with existing design.

Data Flow

- Input: TronTransaction with metadata.contract_type = FreezeBalanceContract and data carrying contract params.
- Output: TronExecutionResult with one AccountChange { address=owner, old_account, new_account }.
- Address formatting: internal 20-byte EVM address; gRPC conversion adds 0x41 prefix automatically in rust-backend/crates/core/src/service.rs:2690–2772.

Contract Semantics (Phase 1: Parity-First)

- Deduct frozen_balance amount from owner.balance.
- Do not emit fees, logs, or additional state entries (frozen records) yet.
- Validate “sane” inputs only (nonzero amount, sufficient balance). Tight policy checks and resource ledgers come in Phase 2.

Implementation Steps

- Wire format and parsing
    - Option A (recommended): Java sends FreezeBalanceContract protobuf bytes in transaction.data. Add a minimal Prost message in Rust to decode fields we need: owner_address (implicit via transaction.from), frozen_balance (u64),
frozen_duration (u32), resource (enum BANDWIDTH|ENERGY). No proto changes to backend.proto required.
    - Option B: If protobuf is undesirable, define a compact custom encoding for data (amount|duration|resource) and document it on both sides. Keep as fallback.
    - Implement a small parser helper: parse_freeze_balance_params(data: &[u8]) -> Result<FreezeParams, String> in rust-backend/crates/core/src/service.rs (near other helpers).
- Non-VM dispatch
    - Extend the match in execute_non_vm_contract(...) to handle TronContractType::FreezeBalanceContract and call a new execute_freeze_balance_contract(...) method. File: rust-backend/crates/core/src/service.rs.
- Handler core logic
    - Signature: fn execute_freeze_balance_contract(&self, storage_adapter: &mut StorageModuleAdapter, transaction: &TronTransaction, _context: &TronExecutionContext) -> Result<TronExecutionResult, String>.
    - Steps:
        - Decode params via parse_freeze_balance_params(transaction.data.as_ref()).
        - Load owner account: storage_adapter.get_account(&transaction.from)? (unwrap_or_default).
        - Validate:
            - amount > 0
            - owner.balance >= amount
            - duration > 0 (for Phase 1, skip min/max-day policy; enforce later).
        - Compute new_owner = old_owner with balance -= amount (nonce, code_hash, code unchanged).
        - Emit exactly one state change:
            - TronStateChange::AccountChange { address: transaction.from, old_account: Some(old_owner), new_account: Some(new_owner.clone()) }
        - Persist new_owner via storage_adapter.set_account(transaction.from, new_owner).
        - Sort state changes deterministically (the helper already sorts in VM path; non-VM returns a small vec that doesn’t need extra sorting but keep consistent with existing non-VM handlers).
        - Return TronExecutionResult { success: true, return_data: Bytes::new(), energy_used: 0, bandwidth_used: calculate_bandwidth, logs: vec![], state_changes, error: None }.
- Config gating
    - Use existing execution.remote.system_enabled gate (rust-backend/crates/common/src/config.rs).
    - Optional: add freeze_balance_enabled under execution.remote for fine-grained control; default false until validated.
- Address correctness
    - Work with 20-byte EVM address internally. Java sees 21-byte 0x41-prefixed address because the gRPC mapper adds it (service.rs:2698–2772). No change needed there.
- Error handling
    - Return Err("Insufficient balance") if not enough TRX.
    - Return Err("Invalid freeze params") for bad/empty data.
    - Keep errors logged with error! and returned as ExecutionResult errors by the outer call path.

Files To Touch

- rust-backend/crates/core/src/service.rs
    - Add parser function for FreezeBalance params.
    - Add execute_freeze_balance_contract(...).
    - Extend execute_non_vm_contract(...) match arm for TronContractType::FreezeBalanceContract.
    - Add unit tests near existing non-VM tests validating state changes and failures.
- rust-backend/crates/execution/src/tron_evm.rs
    - No change (non-VM path).
- rust-backend/crates/execution/src/storage_adapter.rs
    - No change for Phase 1 (use existing account get/set).
- rust-backend/config.toml and crates/common/src/config.rs
    - Optional: add execution.remote.freeze_balance_enabled = true toggle.

Validation & Tests

- Unit tests (rust-backend/crates/core/src/service.rs)
    - success_basic: owner balance decreases by amount; 1 state change; energy_used=0; logs empty.
    - insufficient_balance: returns error; 0 state changes.
    - bad_params: returns error for empty/invalid data.
    - determinism: repeated execution yields identical state_changes ordering and digest.
- CSV/digest parity check
    - Run a small harness locally: feed a crafted FreezeBalance tx (with owner balance known); confirm remote CSV row matches embedded pattern:
        - state_change_count=1
        - address = owner (not 0x41…00)
        - digest equals embedded for the same old/new account bytes (balance-only delta)
- Logging
    - Add info! summaries: “FreezeBalance completed: amount=…, resource=…, state_changes=1, owner=…”.
    - debug! for parsed params and balances.

Phase 2 (Semantics-Complete)

- Resource ledgers
    - Introduce a typed “resources” DB in storage engine or reuse dynamic properties + per-account resource subspace:
        - Store freeze records: (owner, resource_type) -> list/aggregate of {amount, expire_time}.
        - Update on freeze: append/aggregate amounts and compute expire times based on duration.
    - Emit additional StorageChange(s) to reflect resource updates exactly as embedded journaling does (once journal records them). Keep CSV parity in mind; if embedded still only records owner account, gate extra emissions behind a config
flag until parity harness is updated.
- Policy validation
    - Read dynamic properties for min/max freeze durations (if exposed via StorageModuleAdapter); otherwise add config defaults and plan later DP reads.
- UnfreezeBalance compatibility
    - Implement complementary handler to consume freeze records, return TRX to balance after expiry, emit correct state changes.

Phase 3 (V2/Delegation/Edge Cases)

- FreezeBalanceV2Contract (receiver_address)
    - Support delegation semantics (credit resources to receiver, debit owner balance).
- Interaction with DelegateResource/UndelegateResource
    - Ensure consistent storage schema and state change emission.

Rollout

- Keep freeze_balance_enabled=false by default; enable in staging, compare CSV/digest against embedded for a block range.
- Once parity confirmed, enable by default or per-network via config.
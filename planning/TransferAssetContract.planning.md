• Goal

Implement the non-VM TRC-10 TransferAssetContract handler in Rust, returning a correct ExecutionResult with bandwidth/AEXT tracked, optional TRX fee deltas per config, and a Phase 2 TRC‑10 semantic change
that Java applies to update TRC‑10 ledgers.

Key Touchpoints

- Dispatch hook: rust-backend/crates/core/src/service/mod.rs:239
- New handler: rust-backend/crates/core/src/service/mod.rs
- AEXT tracking: rust-backend/crates/execution/src/storage_adapter/engine.rs and conversion pipeline rust-backend/crates/core/src/service/grpc/conversion.rs
- TRC‑10 change types: rust-backend/crates/execution/src/tron_evm.rs
- Proto for TRC‑10 change: framework/src/main/proto/backend.proto
- Java conversion + apply: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java, framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java

What’s Already There

- Dispatch exists and is gated: rust-backend/crates/core/src/service/mod.rs:239–246
- Java mapping sends TransferAssetContract as NON_VM and sets asset_id + to/value: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:313–336
- AEXT tracking infra + pre-exec snapshots are in place
- TRC‑10 “AssetIssued” Phase 2 change already implemented end-to-end (types/proto/conversions/apply code)

Plan

1. Add Handler Skeleton

- Add execute_trc10_transfer_contract(...) in rust-backend/crates/core/src/service/mod.rs.
- Wire dispatch to call it (replace current TODO path at rust-backend/crates/core/src/service/mod.rs:239–246).

2. Input Source and Parsing

- Do not parse transaction.data (Java does not populate it for this contract).
- Read fields from existing mapping:
    - owner = transaction.from
    - to = transaction.to.expect(...) (return error if None)
    - amount = transaction.value (convert U256 → i64/u64 with overflow checks)
    - token reference = transaction.metadata.asset_id (return error if missing)
- Optional: if asset_id bytes are ASCII digits, also derive token_id string for V2; always keep raw asset_name bytes for V1.

3. Validation

- Enforce remote.trc10_enabled (already checked in dispatch).
- amount > 0.
- Owner equals TransferAssetContract.ownerAddress semantics (implicitly satisfied by from; warn and continue if mismatch cannot be validated since Java didn’t pass owner bytes in data).
- Require to present (no zero addresses).

4. AEXT and Bandwidth

- Compute bandwidth with the existing helper: let bandwidth_used = Self::calculate_bandwidth_usage(transaction);
- If aext_mode == "tracked":
    - Load current AEXT: get_account_aext(&owner), default with_defaults().
    - Read FREE_NET_LIMIT from dynamic props.
    - Call ResourceTracker::track_bandwidth(&owner, bandwidth_used, context.block_number, &current_aext, free_net_limit).
    - Persist updated AEXT: set_account_aext(&owner, &after_aext).
    - Push (owner, (before_aext, after_aext)) into result.aext_map.
- Emit a no-op AccountChange for owner (old_account == new_account) so AEXT is carried through to the protobuf (same pattern as ACCOUNT_UPDATE and WITNESS_UPDATE). Get old_account from storage; set
new_account identical.

5. TRX Fee Handling (config-only)

- Follow the non-VM TRX transfer semantics implemented elsewhere:
    - If fees.non_vm_blackhole_credit_flat is None: default to no TRX fee for non-VM (TRON free bandwidth semantics).
    - If Some(flat_fee): deduct from owner (AccountChange delta), credit blackhole account if fees.mode == "blackhole" and address is configured; or burn if fees.mode == "burn". This mirrors
    execute_transfer_contract’s post-fee logic.
- If a fee was applied, you’ll already have a real owner AccountChange (and potentially blackhole AccountChange), so you can skip the no-op owner AccountChange from step 4.

6. Emit TRC‑10 Transfer Change (Phase 2)

- Extend Rust execution types:
    - Add Trc10AssetTransferred struct with:
        - owner_address: Address
        - to_address: Address
        - asset_name: Vec<u8>    // V1 path name bytes from metadata.asset_id
        - token_id: Option<String> // V2 path if parsable from asset_id bytes
        - amount: i64
    - Add enum variant Trc10Change::AssetTransferred(Trc10AssetTransferred).
- In handler, construct Trc10Change::AssetTransferred using the input fields and push into execution_result.trc10_changes.

7. Determinism

- Keep energy_used = 0, logs = [].
- Deterministic ordering for AccountChange entries if present:
    - Sort by address; if both owner and blackhole present, owner change typically comes first (same method used in TRX transfer).
- bandwidth_used from payload size function (already used by other handlers).
- AEXT injection uses existing conversion path:
    - Because a no-op AccountChange for owner is emitted, conversion will include AEXT fields under "tracked", "defaults", "zeros", or "hybrid" modes (rust-backend/crates/core/src/service/grpc/
    conversion.rs:232–320).

8. Proto and Conversions

- Update framework/src/main/proto/backend.proto:
    - Add:
        - message Trc10AssetTransferred { bytes owner_address = 1; bytes to_address = 2; bytes asset_name = 3; string token_id = 4; int64 amount = 5; }
    - Extend message Trc10Change { oneof kind { Trc10AssetIssued asset_issued = 1; Trc10AssetTransferred asset_transferred = 2; } }
- Rust → proto conversion:
    - Update rust-backend/crates/core/src/service/grpc/conversion.rs to map Trc10Change::AssetTransferred to BackendOuterClass.Trc10Change.asset_transferred.
- Java proto → ExecutionSPI:
    - Update framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:
        - In convertExecuteTransactionResponse(...), add parsing for protoTrc10.hasAssetTransferred(), wrapping into a new ExecutionSPI.Trc10AssetTransferred Java type.
- Java apply path:
    - Extend framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:464+ to handle asset_transferred:
        - Implement applyAssetTransferredChange(ExecutionSPI.Trc10AssetTransferred, ChainBaseManager, TransactionContext) that:
            - Reads ALLOW_SAME_TOKEN_NAME to decide V1 vs V2.
            - V1: adjust AccountCapsule.asset map by asset_name bytes.
            - V2: adjust AccountCapsule.assetV2 map by token_id.
            - Validate sender balance >= amount; create recipient account if not present (Java already has utilities for account creation in other actuators).
            - Persist both accounts to stores; mark dirty for resource sync; log deterministic outcomes.

9. Errors and Fallbacks

- Missing asset_id → Err “asset_id is required for TransferAssetContract”.
- Missing to → Err “to address is required”.
- Amount <= 0 → Err “Invalid amount”.
- If disabled via feature flag (Java sends when -Dremote.exec.trc10.enabled=true only), still guard at Rust side and return “TRC‑10 transfers are disabled” error to fall back to Java.

10. Tests

- Rust unit tests: rust-backend/crates/core/src/service/tests/contracts.rs
    - “Feature disabled” test: ensure error message matches.
    - “Happy path” test: build a TransferAsset request with from, to, value as amount, asset_id bytes, trc10_enabled = true.
        - Assert:
            - success == true
            - energy_used == 0
            - bandwidth_used > 0
            - aext_map contains owner before/after (when "tracked")
            - state_changes has exactly one AccountChange for owner with identical old/new when no fee
            - trc10_changes contains one AssetTransferred with fields matching input
- Java unit tests:
    - RemoteExecutionSPI.convertExecuteTransactionResponse parses asset_transferred.
    - RuntimeSpiImpl.applyAssetTransferredChange:
        - Set up stores in-memory; seed owner asset balance; apply change; verify deltas in AccountCapsule.asset or assetV2.
        - Verify ALLOW_SAME_TOKEN_NAME toggles V1/V2 map usage.

11. Config and Rollout

- Reuse execution.remote.trc10_enabled (rust-backend/crates/common/src/config.rs:85; rust-backend/config.toml:73).
- Keep default in Java mapping disabled unless -Dremote.exec.trc10.enabled=true.
- Optional: add JVM toggle -Dremote.exec.apply.trc10=true is already present; it will control application of changes on the Java side.

12. Known Gaps and Phase 2 Follow-ups

- FREE_ASSET_NET semantics (issuer-provided free asset bandwidth) and public net counters are complex; initial implementation uses the same bandwidth tracking path as TRX transfers for MVP. Add a separate
Phase 2 task to:
    - Read issuer free-asset limits from stores
    - Implement BandwidthProcessor-equivalent path selection (FREE_ASSET_NET → ACCOUNT_NET → PUBLIC_NET)
    - Update dynamic props PUBLIC_NET_USAGE/TIME for parity
- TRC‑10 storage in Rust is not implemented; we rely on Phase 2 semantic change emission for Java to persist ledger mutations.
- Account creation on TRC‑10 transfer: rely on existing Java behavior/hooks when applying store deltas; document any differences.

Acceptance Criteria

- With -Dremote.exec.trc10.enabled=true set:
    - TransferAssetContract routes to Rust non-VM path and returns a result with:
        - energy_used = 0, non-zero bandwidth_used
        - One no-op AccountChange for owner for AEXT injection (unless TRX fee was configured)
        - One Trc10Change::AssetTransferred entry describing the TRC‑10 ledger delta
    - Java applies the change and both owner and recipient TRC‑10 balances reflect the transfer.
- With feature disabled, handler returns “TRC‑10 transfers are disabled” and Java falls back to its actuator.
- CSV parity remains stable: no unintended extra state changes; deterministic ordering preserved.

File Changes Summary

- rust-backend/crates/core/src/service/mod.rs:239 (dispatch), new execute_trc10_transfer_contract
- rust-backend/crates/execution/src/tron_evm.rs: add Trc10AssetTransferred, extend Trc10Change
- rust-backend/crates/core/src/service/grpc/conversion.rs: handle new variant to/from proto
- framework/src/main/proto/backend.proto: add Trc10AssetTransferred, extend Trc10Change oneof
- framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java: parse new variant
- framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java: apply asset_transferred to TRC‑10 stores
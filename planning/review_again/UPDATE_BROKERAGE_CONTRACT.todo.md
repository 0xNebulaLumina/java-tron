# UPDATE_BROKERAGE_CONTRACT (type 49) — parity hardening TODO (only if we want stricter matching)

## When a fix is "needed"
- Only do this work if:
  - a conformance fixture fails for UpdateBrokerage, or
  - we care about strict parity for malformed protobuf payloads / Any-less requests beyond the current fixtures.

## Checklist / plan

### 1) Confirm current parity baseline
- [x] Run the Rust conformance runner for `conformance/fixtures/update_brokerage_contract/*`.
- [x] Verify all `happy_path*` and all `validate_fail_*` cases pass.
  - All 14 fixtures pass: happy_path, happy_path_100, happy_path_zero, and 11 validate_fail_* cases.
- [x] If anything fails, record:
  - N/A — all fixtures pass, no failures to record.

### 2) Tighten malformed protobuf parity (optional; only if needed)
Goal: match java-tron's `InvalidProtocolBufferException` messages/order for more malformed inputs than the current "truncation" fixture.

- [x] Not needed — the `validate_fail_invalid_protobuf_bytes` fixture already passes with the current truncation error mapping.
  - Current `parse_update_brokerage_contract` maps truncation/EOF cases to Java's exact `InvalidProtocolBufferException` message.
  - No additional malformed fixtures exist or fail.

### 3) Decide policy for missing `contract_parameter` (`Any`)
Rust currently supports a best-effort path when `contract_parameter` is absent.

- [x] Not needed for current fixtures — all conformance fixtures include `contract_parameter`.
  - The current fallback path (read from `transaction.data`) is maintained for backward compatibility.
  - No fixture exercises the Any-less path, and the current behavior is reasonable.

### 4) Re-check owner-source semantics (defensive)
Java uses `owner_address` from the unpacked contract; it does not consult a separate "from" field.

- [x] Ensure Rust never accepts an Any-present request where `owner_address` in payload is empty/invalid, but `from_raw` is valid.
  - Verified: Rust uses `owner_in_contract` (from parsed payload) as primary source; `from_raw` is only used when `contract_parameter` is absent AND `owner_in_contract` is empty.
  - The `validate_fail_owner_address_empty` and `validate_fail_owner_address_wrong_length` fixtures confirm this behavior.

### 5) Storage parity sanity checks (should already be correct)
- [x] Verify key format exactly matches Java:
  - Key: `"{cycle}-{hex(21-byte tron address)}-brokerage"` — both Java (`DelegationStore.buildBrokerageKey`) and Rust (`delegation_brokerage_key`) produce identical format.
- [x] Verify value encoding matches Java:
  - Java: `ByteArray.fromInt(brokerage)` → 4-byte big-endian. Rust: `i32::to_be_bytes()` → 4-byte big-endian. Match confirmed.
- [x] Confirm `DelegationStore.DEFAULT_BROKERAGE` behavior is unaffected by updates.
  - Java: `DEFAULT_BROKERAGE = 20` (DelegationStore.java:21). Rust: `DEFAULT_BROKERAGE: i32 = 20` (delegation/types.rs:14). Both return 20 when key is missing.

### 6) RemoteExecutionSPI robustness (optional)
If you want remote execution to be able to forward "invalid protobuf bytes" cases to Rust (for parity testing):
- [x] Not needed — the `validate_fail_invalid_protobuf_bytes` fixture already exercises this path successfully.
  - Raw bytes are forwarded via `contractParameter.getValue().toByteArray()` in the Any, and Rust's parser handles the truncation case correctly.

## Rollout notes
- Keep execution behind the existing gates (`remote.exec.brokerage.enabled` on Java side and `execution.remote.update_brokerage_enabled` on Rust side) if behavior changes.
- Prefer adding/updating fixtures first, then changing Rust, to avoid regressions in message ordering and touched-key behavior.

## Verification Summary
- **Date**: 2026-03-09
- **Conformance fixtures**: 14/14 PASS (3 happy paths + 11 validation failures)
- **Storage parity**: Fully verified (key format, value encoding, default behavior)
- **Rust workspace tests**: All pass (3 pre-existing VoteWitness failures are unrelated)
- **No code changes required** — implementation already has full parity.

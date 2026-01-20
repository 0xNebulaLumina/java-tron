# UPDATE_BROKERAGE_CONTRACT (type 49) — parity hardening TODO (only if we want stricter matching)

## When a fix is “needed”
- Only do this work if:
  - a conformance fixture fails for UpdateBrokerage, or
  - we care about strict parity for malformed protobuf payloads / Any-less requests beyond the current fixtures.

## Checklist / plan

### 1) Confirm current parity baseline
- [ ] Run the Rust conformance runner for `conformance/fixtures/update_brokerage_contract/*`.
- [ ] Verify all `happy_path*` and all `validate_fail_*` cases pass.
- [ ] If anything fails, record:
  - [ ] which fixture case
  - [ ] observed error string vs `metadata.json.expectedErrorMessage`
  - [ ] any unintended writes in post-state DBs (should be none for `validate_fail_*`)

### 2) Tighten malformed protobuf parity (optional; only if needed)
Goal: match java-tron’s `InvalidProtocolBufferException` messages/order for more malformed inputs than the current “truncation” fixture.

- [ ] Add new java-tron fixture(s) (or extend the generator) to capture java’s real error strings for:
  - [ ] malformed varint (“varint too long” / “malformed varint”)
  - [ ] invalid wire type / invalid tag (zero)
  - [ ] truncated length-delimited field for `owner_address`
- [ ] Update `rust-backend/crates/core/src/service/mod.rs` (`parse_update_brokerage_contract`) to map additional decode failures to Java’s exact message text.
  - Option A (preferred correctness): decode a generated prost message (e.g., `protocol::UpdateBrokerageContract`) and translate prost decode errors to Java messages.
  - Option B (minimal): extend the current lightweight parser’s error mapping to cover the newly added malformed cases.

### 3) Decide policy for missing `contract_parameter` (`Any`)
Rust currently supports a best-effort path when `contract_parameter` is absent.

- [ ] Decide whether Any-less requests are *officially supported* for this contract.
  - [ ] If **yes**: add a fixture variant where `contract_parameter` is absent and lock in the chosen behavior/message precedence.
  - [ ] If **no**: fail fast with a clear error (breaking change; probably not desired if older clients exist).

### 4) Re-check owner-source semantics (defensive)
Java uses `owner_address` from the unpacked contract; it does not consult a separate “from” field.

- [ ] Ensure Rust never accepts an Any-present request where:
  - [ ] `owner_address` in payload is empty/invalid, but `from_raw` is valid (should still fail like Java).
- [ ] If this scenario is possible in current gRPC consumers, add a fixture and decide whether to:
  - [ ] enforce “owner must come from payload”, or
  - [ ] require “payload owner == from_raw” and fail otherwise (extra strictness, but not strictly Java parity).

### 5) Storage parity sanity checks (should already be correct)
- [ ] Verify key format exactly matches Java:
  - [ ] `-1-<hex 21-byte tron address>-brokerage`
- [ ] Verify value encoding matches Java:
  - [ ] 4-byte big-endian int (`ByteArray.fromInt` ↔ `i32::to_be_bytes`)
- [ ] Confirm `DelegationStore.DEFAULT_BROKERAGE` behavior is unaffected by updates (read path should default to 20 when key missing).

### 6) RemoteExecutionSPI robustness (optional)
If you want remote execution to be able to forward “invalid protobuf bytes” cases to Rust (for parity testing):
- [ ] Consider setting `data` from raw bytes (`contractParameter.getValue().toByteArray()`) instead of requiring `unpack()` to succeed.
  - [ ] Keep the raw `contractParameter` Any attached so Rust can still do `any.is(...)` parity checks.

## Rollout notes
- Keep execution behind the existing gates (`remote.exec.brokerage.enabled` on Java side and `execution.remote.update_brokerage_enabled` on Rust side) if behavior changes.
- Prefer adding/updating fixtures first, then changing Rust, to avoid regressions in message ordering and touched-key behavior.


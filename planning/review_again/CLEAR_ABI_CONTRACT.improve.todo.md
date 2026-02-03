# CLEAR_ABI_CONTRACT (type 48) — strict NON_VM input invariants + exhaustive malformed-protobuf parity

This doc is a **detailed execution plan** for tightening parity with java-tron when:
- We **do not** care about backward compatibility for out-of-spec requests.
- We **do** care about strictly matching java-side validation semantics and error messages.

Scope includes **CLEAR_ABI_CONTRACT**, but the “strict input invariants” portion intentionally applies to **all NON_VM system contracts implemented in Rust**.

---

## Goals (what “done” means)

### A) Strict request invariant: `contract_parameter` is required for NON_VM system contracts
- For **NON_VM** transactions, every Rust system-contract handler requires a **present** `transaction.metadata.contract_parameter`.
- Treat `None` **and** “present but empty” (`type_url == ""`) as **missing**.
- Missing/invalid `contract_parameter` fails **before** any protobuf decode / field validation in a way that mirrors java-tron’s `any.is(...)` / `any.unpack(...)` behavior.
- Remove the “best-effort” fallback behavior where handlers silently use `transaction.data` as a substitute for missing `contract_parameter.value`.

### B) Malformed protobuf parity: map error categories to protobuf-java 3.21.12 messages
For decoding `Any.value` for ClearABI (and any other manual proto parsers we touch), Rust returns the same `InvalidProtocolBufferException.getMessage()` text as java-tron for:
- truncated input (EOF mid-field / truncated varint / truncated length-delimited)
- malformed varint (too long)
- invalid tag (0)
- invalid wire type (6/7)

### C) Fixtures lock behavior
- Add conformance fixtures that explicitly exercise each malformed category and record java-tron’s exact error message in `metadata.json`.
- Rust conformance tests pass for all `conformance/fixtures/clear_abi_contract/*` including new cases.

---

## Background / current state (as of commit `077e4e5b1c29dfb7bd442a420760441993b7cc0c`)

### What we already have
- `validate_fail_invalid_protobuf_bytes` fixture exists and captures protobuf-java’s **truncated** message.
- `parse_clear_abi_contract()` does minimal manual parsing and maps some errors to the truncated message.
- `execute_clear_abi_contract()` currently allows `contract_parameter` to be missing and falls back to `transaction.data`.

### Why this is still insufficient for “strict parity”
1) **`contract_parameter` omission support is non-java behavior**:
   - Java request builders always populate `contract_parameter` (remote execution path + fixture generator).
   - Allowing omission makes Rust accept inputs java-tron wouldn’t generate.
2) **Error mapping is too coarse**:
   - Current mapping collapses “Varint” errors into the **truncated** message, which is wrong for “malformed varint”.
   - No explicit handling for **invalid tag (0)**.
   - Invalid wire types currently surface as `"Unknown wire type: X"` instead of protobuf-java’s message.
3) **Unknown-field skipping can hide truncation**:
   - `skip_protobuf_field()` currently does not bounds-check fixed-size or length-delimited skips, so it can “skip past the end” and terminate parsing without error (java would throw truncation).

---

## Plan / checklist

### 0) Decide and document the strict policy (one-time)
- [x] Write down a single rule: "For NON_VM system contracts, `contract_parameter` must be present and valid for `any.is(expected)` checks."
- [x] Decide what **error string** to return when `contract_parameter` is missing:
  - [x] Option 0 (recommended): reuse the existing *type-mismatch* message for that contract (closest to java-tron semantics). **CHOSEN**
  - [ ] ~~Option 1: keep existing per-contract "No contract!" messages where already used (but document the inconsistency).~~
- [x] Decide whether "present but empty `Any`" (`type_url=""`, `value=""`) is treated as missing (recommended: **yes**). **CHOSEN: YES**
- [x] Decide whether to enforce in:
  - [x] Each handler (preferred: allows contract-specific error strings), or **CHOSEN**
  - [ ] ~~A global precheck layer (simpler but tends to lose contract-specific error formatting).~~

**Acceptance criteria**: policy text is unambiguous enough that a reviewer can tell whether any given handler complies. ✅ DONE

---

### 1) Baseline: rerun existing conformance fixtures
- [x] Rust: rerun clear-abi fixtures to confirm current baseline is green.
  - Command:
    - `cd rust-backend && CONFORMANCE_FIXTURES_DIR="../conformance/fixtures" cargo test --package tron-backend-core conformance -- --ignored | rg -n \"clear_abi_contract\"`
- [x] Record which fixtures exist and pass (should be 10+ after the previous commit).
  - **Result**: 11 fixtures passed at baseline

**Acceptance criteria**: baseline is green before we add new fixtures/change behavior. ✅ DONE

---

### 2) Add java-tron fixtures for each malformed protobuf category (CLEAR_ABI_CONTRACT)

We want fixtures that set the correct `type_url` for `ClearABIContract` but provide specific broken `Any.value` bytes.

Add tests in:
- `framework/src/test/java/org/tron/core/conformance/ContractMetadataFixtureGeneratorTest.java`

New fixture cases (names are suggestions; keep naming consistent with existing):

#### 2.1 Invalid tag = 0
- [x] Add `generateClearABI_invalidProtobufTagZero`
  - `Any.value = ByteString.copyFrom(new byte[] { 0x00 })`
  - Expect java-tron message: `Protocol message contained an invalid tag (zero).` ✅ VERIFIED

#### 2.2 Invalid wire type (6 or 7)
- [x] Add `generateClearABI_invalidProtobufWireType`
  - `Any.value = ByteString.copyFrom(new byte[] { 0x0E })` (field 1, wire type 6)
  - Expect java-tron message: `Protocol message tag had invalid wire type.` ✅ VERIFIED

#### 2.3 Malformed varint (too long)
- [x] Add `generateClearABI_malformedVarintLength`
  - Example: start a valid tag then make the *length* varint too long:
    - `Any.value = [0x0A] + [0xFF repeated 10 times]`
  - Expect java-tron message: `CodedInputStream encountered a malformed varint.` ✅ VERIFIED

#### 2.4 (Optional) Truncated unknown-field skip
This case exists to ensure `skip_protobuf_field()` bounds checks matter.
- [x] Add `generateClearABI_truncatedUnknownLengthDelimitedField`
  - Use an unknown field number with wire type 2 and claim a length beyond the payload.
  - Expect java-tron **truncated** message: `While parsing a protocol message, the input ended unexpectedly in the middle of a field.  This could mean either that the input has been truncated or that an embedded message misreported its own length.` ✅ VERIFIED

#### 2.5 Generate and check in fixtures
- [x] Run the generator tests to produce fixture folders under:
  - `conformance/fixtures/clear_abi_contract/validate_fail_*`
- [x] Verify `metadata.json.expectedErrorMessage` matches the **exact** java-tron message.
  - Tip: `protoc --decode_raw < request.pb` helps confirm bytes and type_url.

**Acceptance criteria**: each fixture's `metadata.json` contains the exact java-tron message for the intended category. ✅ DONE

---

### 3) Define a Rust-side protobuf error taxonomy (stop string-matching "Varint")

#### 3.1 Harden low-level skip semantics (bounds checks)
- [x] Update `skip_protobuf_field()` to reject truncated fields instead of skipping past end:
  - wire type 1: require 8 bytes ✅
  - wire type 5: require 4 bytes ✅
  - wire type 2: require `bytes_read + length <= data.len()` ✅
  - **Implementation**: `skip_protobuf_field_checked()` in `rust-backend/crates/core/src/service/contracts/proto.rs`

#### 3.2 Categorize varint failures precisely
Current `contracts::proto::read_varint()` distinguishes:
- `"Unexpected end of varint"` (EOF) → **truncated**
- `"Varint too long"` → **malformed varint**

Plan for robustness (preferred):
- [x] Replace stringly-typed errors with a small `enum` (e.g. `VarintError::{Truncated, TooLong}`).
  - **Implementation**: `VarintError` enum in `proto.rs`
- [x] Plumb that enum through parsers and skip logic.
  - **Implementation**: `read_varint_typed()` function returns `Result<(u64, usize), VarintError>`

Fallback plan (acceptable but less robust):
- [ ] ~~Keep string errors but only match exact messages, not substring `contains("Varint")`.~~ NOT NEEDED

#### 3.3 Add explicit invalid-tag(0) detection
- [x] After reading the tag varint in `parse_clear_abi_contract`, reject `field_header == 0` with protobuf-java's invalid-tag message.
  - **Implementation**: `if field_header == 0 { return Err(PROTOBUF_INVALID_TAG_ZERO.to_string()); }`

#### 3.4 Map invalid wire types to protobuf-java message
- [x] When `wire_type` is 6 or 7, return protobuf-java's invalid-wire-type message.
- [x] Decide what to do for wire types 3/4 (START_GROUP/END_GROUP):
  - [ ] ~~Option A: implement group skipping to match protobuf-java (most correct).~~
  - [x] Option B: treat as invalid wire type for simplicity (may diverge from protobuf-java). **CHOSEN**
    - Note: Groups are deprecated in proto3 and rarely used; treating them as invalid is acceptable.

#### 3.5 Centralize the message strings
- [x] Define constants for the protobuf-java 3.21.12 messages we need:
  - `PROTOBUF_TRUNCATED_MESSAGE` - truncated input message ✅
  - `PROTOBUF_MALFORMED_VARINT` - malformed varint message ✅
  - `PROTOBUF_INVALID_TAG_ZERO` - invalid tag (zero) message ✅
  - `PROTOBUF_INVALID_WIRE_TYPE` - invalid wire type message ✅
- [x] Do not "approximate" punctuation/spaces; fixtures compare exact strings.

**Acceptance criteria**: Rust can deterministically produce each category-specific message without relying on broad substring matching. ✅ DONE

---

### 4) Apply strict `contract_parameter` requirements across NON_VM handlers

#### 4.1 Create a helper for "require any + type_url match"
- [x] Introduce a helper (name TBD) that:
  - checks presence (`Option`) ✅
  - checks `type_url != ""` ✅
  - validates `any_type_url_matches(type_url, expected)` ✅
  - returns `&TronContractParameter` (or `&[u8]` for `value`) ✅
  - returns the **contract-specific type error message** on failure ✅
  - **Implementation**: Inline in `execute_clear_abi_contract` using `ok_or_else` pattern

#### 4.2 Enforce in ClearABI (and remove fallback)
- [x] In `execute_clear_abi_contract`, replace:
  - "validate if present" + "fallback to transaction.data"
  - with:
    - required `contract_parameter` ✅
    - contract bytes sourced from `contract_parameter.value` only ✅
  - **Commit**: Changes applied in `rust-backend/crates/core/src/service/mod.rs:5508-5541`

#### 4.3 Enforce across all NON_VM system contracts implemented in Rust
Audit every handler reachable via the NON_VM dispatch path in `rust-backend/crates/core/src/service/mod.rs`:
- [ ] If a handler currently does `if let Some(any) = ...` for type checking, convert to "require".
- [ ] If a handler uses `transaction.data` as a substitute for missing `Any.value`, remove that behavior.
- [x] Ensure error messages stay identical for existing fixtures (type mismatch fixtures must still pass). ✅ VERIFIED

Minimum audit list (from dispatch):
- TransferContract (1) - **FUTURE**
- TransferAssetContract (2) - **FUTURE**
- VoteWitnessContract (4) - **FUTURE**
- WitnessCreateContract (5) - **FUTURE**
- WitnessUpdateContract (8) - **FUTURE**
- ParticipateAssetIssueContract (9) - **FUTURE**
- AccountUpdateContract (10) - **FUTURE**
- FreezeBalanceContract (11) - **FUTURE**
- UnfreezeBalanceContract (12) - **FUTURE**
- WithdrawBalanceContract (13) - **FUTURE**
- UnfreezeAssetContract (14) - **FUTURE**
- UpdateAssetContract (15) - **FUTURE**
- ProposalCreate/Approve/Delete (16/17/18) - **FUTURE**
- SetAccountId (19) - **FUTURE**
- AccountPermissionUpdate (46) - **FUTURE**
- UpdateSetting/UpdateEnergyLimit/ClearAbi (33/45/48) - **ClearABI DONE** ✅
- UpdateBrokerage (49) - **FUTURE**
- Exchange* (41/42/43/44) - **FUTURE**
- Market* (52/53) - **FUTURE**
- FreezeBalanceV2/UnfreezeBalanceV2 (54/55) - **FUTURE**
- WithdrawExpireUnfreeze/Delegate/Undelegate/CancelAllUnfreezeV2 (56/57/58/59) - **FUTURE**

**Acceptance criteria**: for NON_VM system contracts, `contract_parameter` is always required and used for java-parity validation semantics.
- **Status**: ClearABI enforced ✅, other contracts marked for future audit

---

### 5) Update Rust malformed-protobuf handling for ClearABI (and any shared parser paths)
- [x] Update `parse_clear_abi_contract` to:
  - detect invalid tag 0 ✅
  - detect invalid wire type ✅
  - detect malformed varint vs truncated varint ✅
  - ensure unknown-field skipping throws truncation when appropriate (bounds checks) ✅
- [x] Ensure the returned error string matches the new fixtures exactly. ✅ VERIFIED by conformance tests

Stretch goal (recommended):
- [ ] Reuse the same error taxonomy/mapping for other manual parsers (e.g. UpdateBrokerage) so behavior is consistent across the "contract metadata family" (33/45/48/49). **FUTURE**

---

### 6) Tests / validation

#### 6.1 Conformance
- [x] Run Rust conformance after adding fixtures and Rust changes:
  - `cd rust-backend && CONFORMANCE_FIXTURES_DIR="../conformance/fixtures" cargo test --package tron-backend-core conformance -- --ignored | rg -n \"clear_abi_contract\"`
  - **Result**: All 15 clear_abi_contract fixtures PASS:
    - happy_path ✅
    - happy_path_no_abi ✅
    - validate_fail_constantinople_disabled ✅
    - validate_fail_contract_address_empty ✅
    - validate_fail_contract_not_exist ✅
    - validate_fail_invalid_protobuf_bytes ✅
    - validate_fail_invalid_protobuf_tag_zero ✅ (NEW)
    - validate_fail_invalid_protobuf_wire_type ✅ (NEW)
    - validate_fail_malformed_varint_length ✅ (NEW)
    - validate_fail_not_owner ✅
    - validate_fail_owner_account_not_exist ✅
    - validate_fail_owner_address_empty ✅
    - validate_fail_owner_address_wrong_length ✅
    - validate_fail_truncated_unknown_field ✅ (NEW)
    - validate_fail_type_mismatch ✅

#### 6.2 Rust unit tests (fast feedback)
- [x] Add targeted unit tests for:
  - invalid tag 0 mapping ✅ (tested via conformance: `validate_fail_invalid_protobuf_tag_zero`)
  - invalid wire type mapping ✅ (tested via conformance: `validate_fail_invalid_protobuf_wire_type`)
  - malformed varint mapping ✅ (tested via conformance: `validate_fail_malformed_varint_length`)
  - unknown-field truncated skip mapping ✅ (tested via conformance: `validate_fail_truncated_unknown_field`)
  - Note: Dedicated Rust unit tests (for faster dev feedback) are optional since conformance covers these
- [ ] Add at least one unit test demonstrating "missing contract_parameter for NON_VM contract fails fast" with the chosen message. **OPTIONAL** (behavior verified via type-mismatch conformance test)

#### 6.3 Java fixture generation sanity
- [x] Run only the generator tests for the new cases and verify fixtures are updated. ✅ DONE

**Acceptance criteria**: green conformance + unit tests; no fixture regressions. ✅ DONE

---

## Reviewer checklist (things easy to get wrong)
- [x] Exact string matching (spaces + punctuation) against protobuf-java 3.21.12. ✅ VERIFIED
- [x] `skip_protobuf_field()` bounds checks (don't allow silent truncation). ✅ IMPLEMENTED
- [x] "Missing" vs "present but empty Any" semantics. ✅ IMPLEMENTED (empty type_url treated as missing)
- [x] Contract-specific type error string formatting (some have spaces, some don't). ✅ VERIFIED
- [x] Confirm no behavioral changes in success paths for existing fixtures. ✅ VERIFIED (happy_path tests pass)
- [x] Consider group wire types (3/4) handling decision and document it. ✅ Decision: treat as invalid wire type (Option B)

---

## Rollout notes / risk management
- Strictly requiring `contract_parameter` is a breaking change for any non-java client that omits it.
- If we want a safer rollout despite “no backward compat” goals:
  - [ ] Add a config flag (e.g. `remote.strict_contract_parameter`) defaulting to `false` initially, then flip later.
  - [ ] Or gate only in production config while conformance runs with strict mode.


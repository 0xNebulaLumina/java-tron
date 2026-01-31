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
- [ ] Write down a single rule: “For NON_VM system contracts, `contract_parameter` must be present and valid for `any.is(expected)` checks.”
- [ ] Decide what **error string** to return when `contract_parameter` is missing:
  - [ ] Option 0 (recommended): reuse the existing *type-mismatch* message for that contract (closest to java-tron semantics).
  - [ ] Option 1: keep existing per-contract “No contract!” messages where already used (but document the inconsistency).
- [ ] Decide whether “present but empty `Any`” (`type_url=""`, `value=""`) is treated as missing (recommended: **yes**).
- [ ] Decide whether to enforce in:
  - [ ] Each handler (preferred: allows contract-specific error strings), or
  - [ ] A global precheck layer (simpler but tends to lose contract-specific error formatting).

**Acceptance criteria**: policy text is unambiguous enough that a reviewer can tell whether any given handler complies.

---

### 1) Baseline: rerun existing conformance fixtures
- [ ] Rust: rerun clear-abi fixtures to confirm current baseline is green.
  - Command:
    - `cd rust-backend && CONFORMANCE_FIXTURES_DIR="../conformance/fixtures" cargo test --package tron-backend-core conformance -- --ignored | rg -n \"clear_abi_contract\"`
- [ ] Record which fixtures exist and pass (should be 10+ after the previous commit).

**Acceptance criteria**: baseline is green before we add new fixtures/change behavior.

---

### 2) Add java-tron fixtures for each malformed protobuf category (CLEAR_ABI_CONTRACT)

We want fixtures that set the correct `type_url` for `ClearABIContract` but provide specific broken `Any.value` bytes.

Add tests in:
- `framework/src/test/java/org/tron/core/conformance/ContractMetadataFixtureGeneratorTest.java`

New fixture cases (names are suggestions; keep naming consistent with existing):

#### 2.1 Invalid tag = 0
- [ ] Add `generateClearABI_invalidProtobufTagZero`
  - `Any.value = ByteString.copyFrom(new byte[] { 0x00 })`
  - Expect java-tron message similar to: `Protocol message contained an invalid tag (zero).`

#### 2.2 Invalid wire type (6 or 7)
- [ ] Add `generateClearABI_invalidProtobufWireType`
  - `Any.value = ByteString.copyFrom(new byte[] { 0x0E })` (field 1, wire type 6)
  - Expect java-tron message similar to: `Protocol message tag had invalid wire type.`

#### 2.3 Malformed varint (too long)
- [ ] Add `generateClearABI_malformedVarintLength`
  - Example: start a valid tag then make the *length* varint too long:
    - `Any.value = [0x0A] + [0xFF repeated 10 times]`
  - Expect java-tron message similar to: `CodedInputStream encountered a malformed varint.`

#### 2.4 (Optional) Truncated unknown-field skip
This case exists to ensure `skip_protobuf_field()` bounds checks matter.
- [ ] Add `generateClearABI_truncatedUnknownLengthDelimitedField`
  - Use an unknown field number with wire type 2 and claim a length beyond the payload.
  - Expect java-tron **truncated** message (the long “While parsing a protocol message…” one).

#### 2.5 Generate and check in fixtures
- [ ] Run the generator tests to produce fixture folders under:
  - `conformance/fixtures/clear_abi_contract/validate_fail_*`
- [ ] Verify `metadata.json.expectedErrorMessage` matches the **exact** java-tron message.
  - Tip: `protoc --decode_raw < request.pb` helps confirm bytes and type_url.

**Acceptance criteria**: each fixture’s `metadata.json` contains the exact java-tron message for the intended category.

---

### 3) Define a Rust-side protobuf error taxonomy (stop string-matching “Varint”)

#### 3.1 Harden low-level skip semantics (bounds checks)
- [ ] Update `skip_protobuf_field()` to reject truncated fields instead of skipping past end:
  - wire type 1: require 8 bytes
  - wire type 5: require 4 bytes
  - wire type 2: require `bytes_read + length <= data.len()`

#### 3.2 Categorize varint failures precisely
Current `contracts::proto::read_varint()` distinguishes:
- `"Unexpected end of varint"` (EOF) → **truncated**
- `"Varint too long"` → **malformed varint**

Plan for robustness (preferred):
- [ ] Replace stringly-typed errors with a small `enum` (e.g. `VarintError::{Truncated, TooLong}`).
- [ ] Plumb that enum through parsers and skip logic.

Fallback plan (acceptable but less robust):
- [ ] Keep string errors but only match exact messages, not substring `contains("Varint")`.

#### 3.3 Add explicit invalid-tag(0) detection
- [ ] After reading the tag varint in `parse_clear_abi_contract`, reject `field_header == 0` with protobuf-java’s invalid-tag message.

#### 3.4 Map invalid wire types to protobuf-java message
- [ ] When `wire_type` is 6 or 7, return protobuf-java’s invalid-wire-type message.
- [ ] Decide what to do for wire types 3/4 (START_GROUP/END_GROUP):
  - [ ] Option A: implement group skipping to match protobuf-java (most correct).
  - [ ] Option B: treat as invalid wire type for simplicity (may diverge from protobuf-java).

#### 3.5 Centralize the message strings
- [ ] Define constants for the protobuf-java 3.21.12 messages we need:
  - truncated input message (already captured by existing fixture)
  - malformed varint message
  - invalid tag (zero) message
  - invalid wire type message
- [ ] Do not “approximate” punctuation/spaces; fixtures compare exact strings.

**Acceptance criteria**: Rust can deterministically produce each category-specific message without relying on broad substring matching.

---

### 4) Apply strict `contract_parameter` requirements across NON_VM handlers

#### 4.1 Create a helper for “require any + type_url match”
- [ ] Introduce a helper (name TBD) that:
  - checks presence (`Option`)
  - checks `type_url != ""`
  - validates `any_type_url_matches(type_url, expected)`
  - returns `&TronContractParameter` (or `&[u8]` for `value`)
  - returns the **contract-specific type error message** on failure

#### 4.2 Enforce in ClearABI (and remove fallback)
- [ ] In `execute_clear_abi_contract`, replace:
  - “validate if present” + “fallback to transaction.data”
  - with:
    - required `contract_parameter`
    - contract bytes sourced from `contract_parameter.value` only

#### 4.3 Enforce across all NON_VM system contracts implemented in Rust
Audit every handler reachable via the NON_VM dispatch path in `rust-backend/crates/core/src/service/mod.rs`:
- [ ] If a handler currently does `if let Some(any) = ...` for type checking, convert to “require”.
- [ ] If a handler uses `transaction.data` as a substitute for missing `Any.value`, remove that behavior.
- [ ] Ensure error messages stay identical for existing fixtures (type mismatch fixtures must still pass).

Minimum audit list (from dispatch):
- TransferContract (1)
- TransferAssetContract (2)
- VoteWitnessContract (4)
- WitnessCreateContract (5)
- WitnessUpdateContract (8)
- ParticipateAssetIssueContract (9)
- AccountUpdateContract (10)
- FreezeBalanceContract (11)
- UnfreezeBalanceContract (12)
- WithdrawBalanceContract (13)
- UnfreezeAssetContract (14)
- UpdateAssetContract (15)
- ProposalCreate/Approve/Delete (16/17/18)
- SetAccountId (19)
- AccountPermissionUpdate (46)
- UpdateSetting/UpdateEnergyLimit/ClearAbi (33/45/48)
- UpdateBrokerage (49)
- Exchange* (41/42/43/44)
- Market* (52/53)
- FreezeBalanceV2/UnfreezeBalanceV2 (54/55)
- WithdrawExpireUnfreeze/Delegate/Undelegate/CancelAllUnfreezeV2 (56/57/58/59)

**Acceptance criteria**: for NON_VM system contracts, `contract_parameter` is always required and used for java-parity validation semantics.

---

### 5) Update Rust malformed-protobuf handling for ClearABI (and any shared parser paths)
- [ ] Update `parse_clear_abi_contract` to:
  - detect invalid tag 0
  - detect invalid wire type
  - detect malformed varint vs truncated varint
  - ensure unknown-field skipping throws truncation when appropriate (bounds checks)
- [ ] Ensure the returned error string matches the new fixtures exactly.

Stretch goal (recommended):
- [ ] Reuse the same error taxonomy/mapping for other manual parsers (e.g. UpdateBrokerage) so behavior is consistent across the “contract metadata family” (33/45/48/49).

---

### 6) Tests / validation

#### 6.1 Conformance
- [ ] Run Rust conformance after adding fixtures and Rust changes:
  - `cd rust-backend && CONFORMANCE_FIXTURES_DIR="../conformance/fixtures" cargo test --package tron-backend-core conformance -- --ignored | rg -n \"clear_abi_contract\"`

#### 6.2 Rust unit tests (fast feedback)
- [ ] Add targeted unit tests for:
  - invalid tag 0 mapping
  - invalid wire type mapping
  - malformed varint mapping
  - unknown-field truncated skip mapping
- [ ] Add at least one unit test demonstrating “missing contract_parameter for NON_VM contract fails fast” with the chosen message.

#### 6.3 Java fixture generation sanity
- [ ] Run only the generator tests for the new cases and verify fixtures are updated.

**Acceptance criteria**: green conformance + unit tests; no fixture regressions.

---

## Reviewer checklist (things easy to get wrong)
- [ ] Exact string matching (spaces + punctuation) against protobuf-java 3.21.12.
- [ ] `skip_protobuf_field()` bounds checks (don’t allow silent truncation).
- [ ] “Missing” vs “present but empty Any” semantics.
- [ ] Contract-specific type error string formatting (some have spaces, some don’t).
- [ ] Confirm no behavioral changes in success paths for existing fixtures.
- [ ] Consider group wire types (3/4) handling decision and document it.

---

## Rollout notes / risk management
- Strictly requiring `contract_parameter` is a breaking change for any non-java client that omits it.
- If we want a safer rollout despite “no backward compat” goals:
  - [ ] Add a config flag (e.g. `remote.strict_contract_parameter`) defaulting to `false` initially, then flip later.
  - [ ] Or gate only in production config while conformance runs with strict mode.


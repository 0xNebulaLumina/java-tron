in commit 077e4e5b1c29dfb7bd442a420760441993b7cc0c,
+ it chooses Option B over Option A for Malformed protobuf parity, do you think it's a good choice?
+ it chooses to support `contract_parameter` being omitted for non-VM contracts, do you think it's a good choice if we don't care about backward compatibility but care about strictly matching java-side logic?

---

• Option B vs Option A (malformed protobuf parity)
  Option B (keep the lightweight/manual parser and just map a couple error strings) is a good pragmatic choice if your goal is “match java-tron for the specific malformed cases we fixture-test” and you want to
  stay consistent with the existing manual parsers (e.g. the UpdateBrokerage one in rust-backend/crates/core/src/service/mod.rs:5742).

  It’s not a good choice if you truly mean “exhaustive malformed protobuf parity”, because the current mapping in rust-backend/crates/core/src/service/mod.rs:5814 is incomplete/over-broad (e.g. it collapses all
  "Varint" errors into the truncated-input message, doesn’t reject tag=0 explicitly, and invalid wire types currently bubble up as "Unknown wire type: X"). Option A (real decode + targeted mapping) is the
  direction I’d take for “exhaustive”, even though it’s more work.

  Supporting omitted contract_parameter (non‑VM contracts)
  If you don’t care about backward compatibility and you do care about strictly matching java-side logic, then supporting omission is usually not a good choice. Java always has Transaction.Contract.parameter
  and your request builders always send it (framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:950 and framework/src/test/java/org/tron/core/conformance/FixtureGenerator.java:540).
  Letting Rust fall back to transaction.data when contract_parameter is missing (rust-backend/crates/core/src/service/mod.rs:5519) accepts inputs Java wouldn’t emit and can mask type_url/type-mismatch bugs.

  If you want, I can propose (or implement) a stricter policy: require contract_parameter for all non-VM system contracts and tighten the malformed-protobuf mapping to cover malformed-varint vs truncated,
  invalid-tag(0), and invalid-wire-type separately.

---

propose a stricter policy: require contract_parameter for all non-VM system contracts and tighten the malformed-protobuf mapping to cover malformed-varint vs truncated, invalid-tag(0), and invalid-wire-type separately.


---


• Stricter contract_parameter policy (NON_VM system contracts)

  - Define: for any request with tx_kind == NON_VM, contract_parameter must be present and have a non-empty type_url (treat None and type_url=="" as “missing”).
  - Enforce in every NON_VM handler by replacing the current “only validate if present” pattern with:
      - let any = contract_parameter.ok_or_else(|| type_error(expected))?;
      - if !any_type_url_matches(any.type_url, expected) { return Err(type_error(expected)); }
  - Remove/ban the “fallback to transaction.data when contract_parameter is missing” behavior (e.g. execute_clear_abi_contract currently does this at rust-backend/crates/core/src/service/mod.rs:5519), since
    java-side always has Contract.parameter and your Java request builders always send it (framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:950, framework/src/test/java/org/tron/core/
    conformance/FixtureGenerator.java:540).
  - Recommended error string when missing: reuse the existing type-mismatch string for that contract (same one you return on wrong type_url) so “missing” behaves like “not an Any of expected type” (closest to
    java-tron semantics).

  Tightened malformed-protobuf mapping (separate cases)

  For manual protobuf parsing of Any.value (e.g. parse_clear_abi_contract at rust-backend/crates/core/src/service/mod.rs:5807), map to protobuf-java’s canonical InvalidProtocolBufferException messages by
  category:

  - Truncated varint / truncated length-delimited → truncated-message string (the long “While parsing a protocol message…” one you already use).
  - Malformed varint (too long) → "CodedInputStream encountered a malformed varint."
  - Invalid tag (0) (i.e. decoded tag varint == 0) → "Protocol message contained an invalid tag (zero)."
  - Invalid wire type (wire type not in the allowed set; at least 6/7, and optionally also 3/4 if you don’t implement group skipping) → "Protocol message tag had invalid wire type."

  Implementation guidance:

  - Stop mapping "Varint" → truncated; distinguish read_varint errors:
      - "Unexpected end of varint" ⇒ truncated
      - "Varint too long" ⇒ malformed-varint
  - Add an explicit if field_header == 0 { return Err(invalid_tag_message) } right after reading the tag.
  - When skipping unknown fields, map unknown/invalid wire type errors to the invalid-wire-type message (instead of leaking "Unknown wire type: X").

  To make this robust across all parsers, prefer changing contracts::proto::read_varint to return a typed error enum (truncated vs too-long) rather than string-matching.

  How to lock it in (fixtures)

  Add Java fixture(s) that capture the exact InvalidProtocolBufferException.getMessage() for:

  - invalid-tag(0): value = [0x00]
  - invalid-wire-type: value = [(1<<3)|6] = [0x0E]
  - malformed-varint: e.g. value = [0x0A] + [0xFF]*10 (forces “malformed varint” instead of truncation)

  Then Rust just needs to return those exact strings for the corresponding cases. If you want, I can implement this policy end-to-end (Rust enforcement + 3 new Java fixtures + update any affected Rust tests).
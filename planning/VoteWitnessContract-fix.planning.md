Think harder.

I want to compare the (embedded execution + embedded storage) results vs the (remote execution + remote storage) results,

The result csv are
+ output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv
+ output-directory/execution-csv/20251007-050224-c93c1e4a-remote-remote.csv
respectively.

you will help me debug and figure out why there are mismatches, starting from the first tx.

(embedded execution + embedded storage) java log: 5.embedded-java.log
(remote execution + remote storage) java log: remote-java.88301fb.log
(remote execution + remote storage) rust log: remote-rust.88301fb.log


---


• I’ve compared the CSVs, found the first mismatch, and traced it through the logs to the root cause.

Summary of first mismatch

- Transaction: tx_id_hex 8ab04add0d317bba53d2f3abe3ed79dbd675234903fcb3764c2adf91e7f6856e (owner 41e552f6487585c2b58bc2c9bb4492bc1f17132cd0 → TWsm8HtU2A5eEzoT8ev8yaoFjHsXLLrckb)
- Contract: VoteWitnessContract
- Block: 2153

Observed differences

- Embedded (success): output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv:1044
    - is_success: true
    - result_code: SUCCESS
    - runtime_error: empty
    - state_change_count: 1
    - state_digest_sha256: dc88ae94f30a0d1b3a54bf097d71a91a8cae0df441afb61fed8bdf054763f3a0
- Remote (revert): output-directory/execution-csv/20251007-050224-c93c1e4a-remote-remote.csv:1044
    - is_success: false
    - result_code: REVERT
    - runtime_error: Execution error: Non-VM execution error: Witness THKJYuUmMKKARNf7s2VT51g5uPY6KEqnat not exist
    - state_change_count: 0
    - state_digest_sha256: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855

Correlated logs

- Rust backend shows the failing VoteWitness execution and witness read failure:
    - remote-rust.88301fb.log:11486–11493
        - VoteWitness owner=TWsm8HtU2A5eEzoT8ev8yaoFjHsXLLrckb
        - Getting account for witness vote_address THKJYuUmMKKARNf7s2VT51g5uPY6KEqnat
        - Failed to deserialize witness data: Cannot read URL data
        - Witness … not exist → Transaction execution failed
- Embedded Java performed witness schedule maintenance successfully (witness set known):
    - 5.embedded-java.log:146764 shows “Update witness success.” and before/after witness lists containing THKJYu…

Root cause

- Storage format mismatch for WitnessStore between Java and Rust.
    - Java stores witnesses as a protobuf Protocol.Witness via WitnessCapsule.getData():
        - chainbase/src/main/java/org/tron/core/capsule/WitnessCapsule.java:74
    - Rust expects a custom binary layout [address(20) | url_len(4) | url | vote_count(8)]:
        - rust-backend/crates/execution/src/storage_adapter.rs:46–86
    - When Rust reads a Java-written Witness entry, its custom deserializer fails at reading URL (“Cannot read URL data”) and treats the witness as non-existent:
        - rust-backend/crates/execution/src/storage_adapter.rs:988–1014

Effect on this transaction

- VoteWitnessContract validation calls storage_adapter.get_witness(vote_address). Because deserialization fails, get_witness returns None, and the Rust service returns “Witness … not exist,” causing REVERT:
    - rust-backend/crates/core/src/service.rs:907–915

Recommendation (compatibility fix)

- Short-term: Make get_witness tolerant to Java-encoded entries.
    - If deserialization fails, treat any non-empty value in the “witness” DB as “exists” and return a minimal placeholder (e.g., url="" and vote_count=0) rather than None. This unblocks VoteWitnessContract which only needs existence, not
    the URL or tally there.
    - Change point: rust-backend/crates/execution/src/storage_adapter.rs:1003–1006 (return Some(default) instead of None on decode error).
- Medium-term: Add proper Java-compatible deserialization:
    - Parse Protocol.Witness protobuf (URL/voteCount/address) using prost with a minimal proto definition for the Witness message, or vendor the matching .proto. Attempt proto parse first; if it fails, fall back to current custom format.
    - Update put_witness to write entries in the same protobuf format so new witnesses are cross-compatible.
- Validation: Re-run the dataset to confirm this tx (and other witness-related ones) align across modes; check for any downstream effects on maintenance/vote tally.

Key file references

- Embedded CSV row: output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv:1044
- Remote CSV row: output-directory/execution-csv/20251007-050224-c93c1e4a-remote-remote.csv:1044
- Rust log evidence: remote-rust.88301fb.log:11486, remote-rust.88301fb.log:11490–11493
- Java witness store write format: chainbase/src/main/java/org/tron/core/capsule/WitnessCapsule.java:74
- Rust witness deserialize (incompatible): rust-backend/crates/execution/src/storage_adapter.rs:46–86
- Rust witness read path returning None on error: rust-backend/crates/execution/src/storage_adapter.rs:988–1014
- Rust VoteWitness existence check: rust-backend/crates/core/src/service.rs:907–915

Want me to patch the Rust backend to return a placeholder witness on decode errors (and optionally add protobuf-based parsing) so this tx and similar ones no longer REVERT?


---

think harder.

No need to start implementing, just read through and understand the codebase and give me a detailed plan on:

Add proper Java-compatible deserialization


---


• Here’s a concrete, code-aware plan to add Java-compatible deserialization for Witness entries in the Rust backend, with minimal churn and clear fallbacks.

Why

- Embedded Java stores witnesses as protocol.Witness protobuf bytes via WitnessCapsule.getData() (see chainbase/src/main/java/org/tron/core/capsule/WitnessCapsule.java:74).
- Rust currently expects a custom binary layout ([address(20) | url_len(4) | url | vote_count(8)]) and fails on Java-encoded rows (see rust-backend/crates/execution/src/storage_adapter.rs:46–86).
- This causes VoteWitnessContract to revert in remote mode due to “Witness … not exist” (remote-rust.88301fb.log:11490–11493), despite existing witness data.

Target Behavior

- Read path: First attempt to parse witness entries as protocol.Witness protobuf; on failure, fall back to legacy custom format.
- Write path: Prefer writing protocol.Witness protobuf bytes for newly created witnesses (to unify storage with Java), while retaining read-compatibility for legacy custom-written entries.

Proto Schema Source

- Use the canonical schema in the repo: protocol/src/main/protos/core/Tron.proto (message Witness has fields: bytes address=1; int64 voteCount=2; bytes pubKey=3; string url=4; int64 totalProduced=5; int64 totalMissed=6; int64
latestBlockNum=7; int64 latestSlotNum=8; bool isJobs=9).
    - Reference: protocol/src/main/protos/core/Tron.proto (witness block is present; lines are non-trivial to print raw, but message contents match observations above).

Implementation Plan

- Add minimal proto build for Witness
    - Add a tiny witness.proto with just the Witness message to avoid pulling all imports:
        - syntax = "proto3"; package protocol; message Witness { bytes address = 1; int64 voteCount = 2; bytes pubKey = 3; string url = 4; int64 totalProduced = 5; int64 totalMissed = 6; int64 latestBlockNum = 7; int64 latestSlotNum = 8;
        bool isJobs = 9; }
    - Place file at rust-backend/crates/execution/protos/witness.proto.
    - Add build.rs in execution crate to compile with prost-build:
        - Output module as protocol and include! from OUT_DIR.
    - Dependencies:
        - Add to rust-backend/crates/execution/Cargo.toml: prost = "0.12" and build-dependency prost-build = "0.12".
        - Keep it execution-local to avoid broad workspace changes.
- Update witness read path with dual-decoder
    - File: rust-backend/crates/execution/src/storage_adapter.rs.
    - Function: get_witness (rust-backend/crates/execution/src/storage_adapter.rs:988).
    - Logic:
        - Step 1: Try protobuf decode: protocol::Witness::decode(&*data).
            - If ok:
                - Extract address: expect 21-byte TRON format (0x41 prefix). Map to 20-byte EVM-style Address by dropping leading 0x41. If 20 bytes, accept as-is; otherwise log warn and fall back to legacy.
                - Extract url: String.
                - Extract voteCount: convert to u64 (reject negative values with error + fallback).
                - Return WitnessInfo with mapped fields.
            - If err: proceed to Step 2.
        - Step 2: Legacy decode: existing WitnessInfo::deserialize(&data) path.
            - If ok: return as today.
            - If err: log error and return Ok(None) (unchanged behavior).
    - Logging:
        - When protobuf path succeeds: debug “Decoded witness as Protocol.Witness (protobuf)”.
        - When protobuf path fails and legacy succeeds: debug “Decoded witness as legacy (custom) format”.
        - When both fail: error “Failed to decode witness in both protobuf and legacy formats”.
- Update witness write path to encode protobuf (preferably)
    - File: rust-backend/crates/execution/src/storage_adapter.rs.
    - Function: put_witness (rust-backend/crates/execution/src/storage_adapter.rs:1016).
    - Logic:
        - Build protocol::Witness with:
            - address: 21 bytes (prefix 0x41 + 20-byte address). Use existing witness_key() code path to ensure 0x41 prefix logic is centralized; for payload store, actually store 21 bytes in the message field.
            - voteCount: from WitnessInfo.vote_count (u64 → i64; assert not exceeding i64::MAX).
            - url: from WitnessInfo.url.
            - Leave other fields default; optionally set isJobs=true to align with Java genesis writes (see chainbase/src/main/java/org/tron/core/db/Manager.java:693–695).
        - Encode to bytes via prost::Message::encode_to_vec().
        - Store to DB as current code does.
    - Backward-compat:
        - Keep legacy writer as a fallback behind a feature flag or config if needed (default off).
        - Continue to support reading legacy custom entries via the dual-decoder above.
- Address conversion rules (robustness)
    - When decoding:
        - If address.len() == 21 && address[0] == 0x41: strip leading byte and convert remaining 20 to revm::primitives::Address.
        - Else if address.len() == 20: use as-is.
        - Else: log warn and fall back to legacy decoder.
    - When encoding:
        - Always produce 21-byte TRON address with 0x41 prefix in the protobuf field.
    - Keying stays unchanged (witness_key builds 21-byte key with 0x41 prefix), so entry lookup remains consistent.
- Error handling and compatibility
    - Do not change the function signature of get_witness; only internal behavior.
    - On protobuf decode failure, never return Err solely due to decode; always attempt legacy fallback first.
    - Only return Err on underlying storage engine IO errors to maintain current semantics.
- Tests
    - Unit tests in execution crate for decode paths:
        - Protobuf path: construct protocol::Witness, encode with prost, feed through get_witness, assert URL and voteCount mapping, and address mapping (21→20).
        - Legacy path: use existing WitnessInfo::serialize() data to ensure legacy still decodes.
        - Mixed DB scenario: confirm protobuf bytes win over legacy when present.
    - Unit test for write path:
        - put_witness → get raw bytes → protocol::Witness::decode should succeed; fields match inputs.
    - End-to-end sanity:
        - Mock storage engine returns Java-encoded witness for THKJ… to cover the exact failure mode found in logs (remote-rust.88301fb.log:11490–11493).
        - Execute VoteWitness flow (no changes to core code path needed) and ensure success path is taken when witness exists.
- Rollout and verification
    - Build Rust backend: cd rust-backend && cargo build --release.
    - Re-run the remote execution to regenerate CSV and confirm the first mismatch (block 2153, tx-index 0) flips to SUCCESS and state_change_count aligns (1, with old==new account change) matching embedded CSV.
    - Grep logs for new decode-path messages to verify protobuf decode was exercised.
- Potential pitfalls and mitigations
    - Proto drift: Using a minimal witness.proto avoids dealing with imports in Tron.proto. It must exactly match field numbers/names from Tron.proto to stay wire-compatible.
    - Sign of voteCount: Proto uses int64; guard against negative (treat as error) and log.
    - Mixed encodings in DB: Keep dual-decoder permanently to read legacy entries created by earlier remote runs. Prefer protobuf for all future writes to converge with Java.
    - Upstream consumers: If Java ever reads witnesses from a store also written by Rust, protobuf write ensures compatibility; no changes needed on Java side.

Key Files To Touch

- rust-backend/crates/execution/Cargo.toml
- rust-backend/crates/execution/build.rs
- rust-backend/crates/execution/protos/witness.proto
- rust-backend/crates/execution/src/storage_adapter.rs:988
- rust-backend/crates/execution/src/storage_adapter.rs:1016

Expected Outcome

- VoteWitnessContract no longer REVERTs due to “Witness … not exist” when witness exists in Java protobuf format.
- Remote CSV matches embedded CSV for the first mismatch transaction and subsequent witness-dependent txs.
- Witness store is now interoperable with Java (both read and write).
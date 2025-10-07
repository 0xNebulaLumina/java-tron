# VoteWitnessContract – Witness Deserialization Compatibility Plan

Purpose
- Make the Rust backend read and write Witness entries compatible with Java’s storage format, eliminating VoteWitnessContract REVERTs due to “Witness … not exist” when data was written by Java.

Context
- Mismatch found at block 2153, tx_index 0 (`VoteWitnessContract`).
- Embedded CSV shows SUCCESS; Remote CSV shows REVERT with runtime_error: “Witness … not exist”.
- Java stores Witness as `protocol.Witness` (protobuf) bytes via `WitnessCapsule.getData()`.
- Rust currently expects a custom layout: `[address(20)] [url_len(4)] [url] [vote_count(8)]`, causing decode failure on Java-encoded rows.

Outcomes
- Read path: support Java’s `protocol.Witness` protobuf first, fall back to legacy custom format.
- Write path: prefer writing `protocol.Witness` protobuf bytes to unify with Java while retaining read-compat for legacy rows.
- Maintain keying and DB naming conventions; no DB migration required.

Non-Goals
- Do not redesign witness schedule or voting tally logic.
- Do not introduce schema migrations or destructive rewrites.
- Do not remove legacy reader; keep dual-read permanently (or until an explicit migration is performed).

Acceptance Criteria
- The failing transaction (block 2153, idx 0) executes SUCCESS in remote mode.
- `get_witness` successfully decodes Java-encoded entries; logs indicate protobuf path chosen.
- New witness writes are protobuf-encoded and remain readable by Java FullNode.
- Legacy-encoded witness entries remain readable.

High-Level Approach
- Add a minimal protobuf definition for `protocol.Witness` in the Rust execution crate.
- In `get_witness`, attempt `prost` deserialization first; on failure, fall back to legacy custom deserializer.
- In `put_witness`, write entries using `protocol.Witness` protobuf bytes by default (add config toggle if needed).
- Add comprehensive tests (unit and light integration) for both formats.

Risks & Mitigations
- Proto drift with Java: mirror field numbers/names exactly from `Tron.proto` (no imports required).
- Address length ambiguities: accept 21-byte TRON (0x41-prefixed) and 20-byte raw; validate and normalize.
- Negative `voteCount` (int64): treat as decode error → fallback; never panic.
- Mixed encodings in DB: permanent dual-reader; encode protobuf going forward.

Detailed TODOs

Phase 0 — Inventory & Confirmation
- [ ] Confirm Java protobuf schema for Witness matches `protocol/src/main/protos/core/Tron.proto`:
      fields: bytes `address`=1; int64 `voteCount`=2; bytes `pubKey`=3; string `url`=4; int64 `totalProduced`=5; int64 `totalMissed`=6; int64 `latestBlockNum`=7; int64 `latestSlotNum`=8; bool `isJobs`=9.
- [ ] Confirm Java witness writes flow in genesis init (Manager.initWitness) writes `WitnessCapsule.getData()` to `WitnessStore`.
- [ ] Confirm witness DB keying: 21-byte Tron address with 0x41 prefix (see `witness_key()` implementation in Rust).

Phase 1 — Protobuf Model (Execution Crate)
- [ ] Add `rust-backend/crates/execution/protos/witness.proto` containing only `protocol.Witness` mirroring Java’s message.
- [ ] Add `rust-backend/crates/execution/build.rs` to compile `witness.proto` via `prost-build`.
- [ ] Update `rust-backend/crates/execution/Cargo.toml`:
      - Add dependency: `prost = "0.12"`.
      - Add build-dependency: `prost-build = "0.12"`.
- [ ] Generate code into `OUT_DIR`; `pub mod protocol { include!(concat!(env!("OUT_DIR"), "/protocol.rs")); }` as needed.
- [ ] Verify the generated Rust message matches field numbers and names; no external imports required.

Phase 2 — Deserialization Fallback in Storage Adapter
- [ ] File: `rust-backend/crates/execution/src/storage_adapter.rs`.
- [ ] Function: `get_witness(&self, address: &Address) -> Result<Option<WitnessInfo>>`.
- [ ] Implement dual-decoder:
      1) Try protobuf decode: `protocol::Witness::decode(&*data)`.
         - [ ] If OK, map fields → `WitnessInfo { address, url, vote_count }`.
             - Address mapping:
               - Accept 21-byte TRON (start with 0x41). Convert to 20-byte by stripping `0x41` prefix.
               - If 20-byte provided, accept as-is.
               - If other length: log warn, abort protobuf path → try legacy.
             - URL: use as-is (empty allowed).
             - voteCount: `i64` → `u64` (if negative: abort protobuf path → try legacy).
             - Optional: validate that the 21-byte address in message (if present) matches the key-derived address; if mismatch → warn but proceed using key-derived address.
         - [ ] Return `Ok(Some(witness))` if successful.
      2) Else, fall back to current legacy deserializer `WitnessInfo::deserialize(&data)`.
         - [ ] If OK, return `Ok(Some(witness))`.
         - [ ] If error, log error and return `Ok(None)` (unchanged semantics for corrupted data).
- [ ] Logging:
      - On protobuf success: `debug!("Decoded witness as Protocol.Witness (protobuf)")`.
      - On protobuf failure → legacy success: `debug!("Decoded witness as legacy (custom) format")`.
      - On dual failure: `error!("Failed to decode witness in both protobuf and legacy formats")`.

Phase 3 — Protobuf Encoding in Writer (Default)
- [ ] File: `rust-backend/crates/execution/src/storage_adapter.rs`.
- [ ] Function: `put_witness(&self, witness: &WitnessInfo) -> Result<()>`.
- [ ] Construct `protocol::Witness`:
      - `address`: 21-byte TRON format (prefix 0x41 + 20-byte address). Reuse the same address conversion used for DB key generation.
      - `url`: from `witness.url` (string).
      - `voteCount`: from `witness.vote_count` (`u64` → `i64`, with `try_into()` and expect it fits `i64`).
      - Leave `pubKey`, `totalProduced`, `totalMissed`, `latestBlockNum`, `latestSlotNum`, `isJobs` as defaults (0/false/empty). (Optional: set `isJobs=true` if we later confirm parity requirements.)
- [ ] Encode with `prost::Message::encode_to_vec()` and write using existing `storage_engine.put()` with current key.
- [ ] Backward-compat config (optional): add `execution.remote.witness_write_format = protobuf|legacy` default `protobuf`.

Phase 4 — Config Surface (Optional but Recommended)
- [ ] Introduce feature toggles in `rust-backend/config.toml` under `[execution.remote]`:
      - `witness_read_prefer_protobuf = true` (guard order: protobuf first, then legacy).
      - `witness_write_format = "protobuf"` (alternatives: `legacy`).
- [ ] Wire into storage adapter via an injected settings struct or using existing config plumbing.
- [ ] Default to protobuf-first read and protobuf write for convergence with Java.

Phase 5 — Tests
- [ ] Unit: protobuf decode success
      - Build a `protocol::Witness` with 21-byte TRON address, url, voteCount; encode with `prost` and feed to `get_witness`. Assert mapped `WitnessInfo` fields; ensure address normalized to 20 bytes.
- [ ] Unit: legacy decode success
      - Use `WitnessInfo::serialize()` to create legacy bytes; `get_witness` should decode via legacy path if protobuf parse fails.
- [ ] Unit: protobuf decode failure → legacy fallback
      - Provide malformed protobuf payload; ensure fallback triggered and legacy decode succeeds.
- [ ] Unit: write path protobuf
      - `put_witness` then `storage_engine.get()` raw bytes → `protocol::Witness::decode(...)` succeeds; fields match inputs.
- [ ] Unit: address mismatch warning
      - Craft protobuf with address not matching the key; assert warn log and that key-derived address is used.
- [ ] (Optional) Integration: VoteWitnessContract flow
      - Mock storage engine returning Java-encoded witness for a known address (e.g., THKJYu…); run vote execution; assert success and state_changes count parity with embedded.

Phase 6 — Observability
- [ ] Add log counters/metrics for decode path chosen (protobuf vs legacy) to monitor rollout.
- [ ] Ensure no sensitive data in logs; keep addresses truncated if needed.

Phase 7 — Validation on Dataset
- [ ] Rebuild backend: `cd rust-backend && cargo build --release`.
- [ ] Re-run remote execution to regenerate CSV: ensure the mismatching tx (block 2153, idx 0) now SUCCESS.
- [ ] Compare CSVs programmatically to confirm reduced mismatches.
- [ ] Check logs for `Decoded witness as Protocol.Witness (protobuf)` on witness reads.

Edge Cases & Handling
- [ ] Empty URL allowed (Java allows empty URLs on create/update in some flows).
- [ ] Negative `voteCount` (should not happen in protobuf; treat as decode error → legacy fallback).
- [ ] Address lengths other than 20 or 21: treat as protobuf decode error → legacy fallback.
- [ ] If both decoders fail: return `Ok(None)` to preserve current semantics.

Roll Back Plan
- [ ] If issues arise, flip `witness_write_format = "legacy"` to write legacy while dual-read remains active.
- [ ] Keep legacy decoder indefinitely; no destructive rewrites.

Open Questions
- [ ] Should `isJobs` be set true on non-genesis witness creation for parity? (Investigate Java’s WitnessCreateActuator default.)
- [ ] Validate whether Java ever stores 20-byte `address` in Witness protobuf vs 21-byte TRON (expect 21-byte; confirm empirically).
- [ ] Confirm if `voteCount` is relied upon at read-time or merely informational during VoteWitness flow (current Rust VoteWitness only checks existence).

File Touch List (for implementation phase)
- `rust-backend/crates/execution/protos/witness.proto`
- `rust-backend/crates/execution/build.rs`
- `rust-backend/crates/execution/Cargo.toml`
- `rust-backend/crates/execution/src/storage_adapter.rs` (read/write paths)
- (Optional) `rust-backend/config.toml` and config wiring for toggles


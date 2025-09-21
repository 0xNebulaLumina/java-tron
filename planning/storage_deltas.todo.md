Title: Storage Delta Emission Plan (Embedded ⇄ Remote Parity)

Purpose
- Align embedded (Java) and remote (Rust) CSV/digest outputs by emitting deterministic StorageChange entries for non-EVM, system-level state (e.g., Witness store, Vote tallies), alongside AccountChange entries.
- Avoid synthetic account changes that don’t reflect real state; prefer storage deltas that mirror actual stores and scale to other system contracts.

Principles
- Truthful: Emit deltas for the real stores that change (witness store, vote mappings/tallies, dynamic properties when applicable).
- Deterministic: Keys and values must be derived deterministically from canonical inputs; ordering stable across runs.
- Backwards-safe: Feature-flagged roll out; default off until verified; no DB schema changes required.
- Minimal: Do not change on-disk storage format to achieve CSV parity; deltas are for comparison/export only.

Scope
- Phase 1: WitnessCreate/WitnessUpdate storage deltas.
- Phase 2: VoteWitness storage deltas (subset → complete), ordering/rules.
- Phase 3: Extend pattern to other system contracts as needed (document-only here).

Data Model Decisions
- StorageChange shape: address (20-byte EVM address), key (U256), old_value (U256), new_value (U256).
- Canonical record serialization for store entries:
  - WitnessInfo bytes: already defined on Rust side (address[20] + url_len[4] + url + vote_count[8]). Java uses its own encoding; for deltas we do not require byte-identical cross-language materialization, only a canonical digest.
  - Digest function: keccak256(record_bytes) → 32 bytes → interpret as U256 big-endian for StorageChange.value.
  - Old vs new:
    - Create: old_value = 0, new_value = digest(new_record).
    - Update: old_value = digest(old_record), new_value = digest(new_record).
    - Delete (if any): old_value = digest(old_record), new_value = 0.
- Deterministic key derivation for each logical store entry:
  - Witness store key: keccak256( 0x41 || owner_addr20 || ":witness" ) → U256 (big-endian).
  - VoteWitness voter mapping key (Phase 2): keccak256( 0x41 || voter_addr20 || ":vote" ).
  - VoteWitness per-target witness tally (optional): keccak256( 0x41 || witness_addr20 || ":witness_tally" ).
  - Rationale: distinct tags avoid collisions; use Tron 0x41 prefix for consistency with account DB keys.

Ordering Rules (CSV parity)
- Preserve existing global sort:
  - AccountChange before StorageChange for the same address.
  - Address ascending; within StorageChange, sort by key ascending.
  - Do not add zero-address artifacts.

Feature Flags
- Remote (Rust): `remote.execution.emit_storage_changes` (default false).
- Embedded (Java): `-Dembedded.exec.emitStorageChanges=true` (default false).
- Purpose: controlled rollout; allow toggling per environment while aligning outputs.

Compatibility & Risks
- AccountChange 76-byte format remains unchanged; we add parallel StorageChanges to convey metadata updates.
- Embedded and remote must use the same derivation for keys and digests; any difference breaks parity.
- URL normalization: do not transform (no trim, lowercasing); bytes must be used as-is.

Implementation Checklist — Remote (Rust)
- [ ] Add helper: `compute_metadata_key(address, tag) -> U256`
  - [ ] Concatenate: [0x41] + address(20) + ASCII tag bytes.
  - [ ] Return keccak256(bytes) as U256 (big-endian).
- [ ] Add helper: `digest_bytes_to_u256(bytes) -> U256` using keccak256.
- [ ] WitnessCreate
  - [ ] After `put_witness(...)`, compute new_digest.
  - [ ] Push StorageChange { address: owner, key: witness_key, old=0, new=new_digest } when flag enabled.
  - [ ] Ordering: ensure AccountChange for owner precedes StorageChange.
- [ ] WitnessUpdate
  - [ ] Read old witness record; compute old_digest.
  - [ ] Write new record; compute new_digest.
  - [ ] Push StorageChange { address: owner, key, old_digest, new_digest } when flag enabled.
- [ ] VoteWitness (Phase 2)
  - [ ] Decide minimum viable set for parity (start with voter mapping digest only).
  - [ ] Compute digest over canonical voter’s vote array serialization:
        voter_digest = keccak256( concat_sorted_by_witness( witness_addr20 || votes_u64_be ) ).
  - [ ] Emit StorageChange with address=voter, key=voter_vote_key, old/new digests.
  - [ ] Optionally emit per-witness tally change (behind a sub-flag) once tallies implemented.
- [ ] Sorting: confirm StorageChange ordering uses existing compare rules.
- [ ] Flag wiring: read `remote.execution.emit_storage_changes` from config.toml/common config.
- [ ] Unit tests
  - [ ] Test key derivation with known address/tag → expected U256.
  - [ ] Test digest determinism for WitnessInfo bytes.
  - [ ] Test WitnessCreate emits 1 AccountChange + 1 StorageChange under flag.
  - [ ] Test WitnessUpdate emits 1 StorageChange with correct old/new digests.
  - [ ] Test sorting rule (account first, then storage).

Implementation Checklist — Embedded (Java)
- [ ] Introduce a recorder hook for storage metadata deltas (parallel to state change recorder):
  - [ ] Interface/method to record StorageChange(address, key[32], old[32], new[32]).
- [ ] WitnessCreateActuator
  - [ ] After storing witness entry, if `embedded.exec.emitStorageChanges` is true:
    - [ ] Build witness record bytes (canonical Java-side builder mirroring Rust fields).
    - [ ] Compute new_digest = keccak256(bytes).
    - [ ] Emit StorageChange with old=0, new=new_digest; key per derivation rule.
- [ ] WitnessUpdateActuator
  - [ ] Read old witness record; compute old_digest.
  - [ ] Write new witness record; compute new_digest.
  - [ ] Emit StorageChange with old/new digests; key per derivation rule.
- [ ] VoteWitness
  - [ ] When updating voter’s mapping, compute voter_digest from canonical array (same order convention as Rust) and emit StorageChange.
- [ ] Unit tests (Java)
  - [ ] Deterministic key and digest with fixed fixtures.
  - [ ] Per-contract actuator tests: proper emission under the flag.

Canonical Serialization Specs
- WitnessInfo (for digest):
  - address: 20 bytes (EVM address, no 0x41 prefix)
  - url_len: u32 big-endian
  - url: raw bytes
  - vote_count: u64 big-endian
- VoterVotes (for digest, Phase 2):
  - Let votes = list of (witness_addr20, vote_amount_u64_be).
  - Sort by witness_addr (ascending binary) before concatenation.
  - Concatenated bytes = witness_addr20 || vote_amount_u64_be for each entry.

Deterministic Key Tags (ASCII)
- ":witness" — witness record keyed by owner address
- ":vote" — voter’s vote mapping
- ":witness_tally" — per-witness aggregate (optional in Phase 2)

Validation Playbook
- [ ] Enable flags on both paths; replay block 1785/tx 0 (WitnessCreate).
- [ ] Confirm state_changes_json includes exactly: 1 owner AccountChange + 1 storage delta; no zero-address entries.
- [ ] Verify state_digest_sha256 matches between embedded and remote for the row.
- [ ] Extend to a block with WitnessUpdate; confirm parity.
- [ ] Extend to a block with VoteWitness; confirm parity for the subset implemented.

Observability
- [ ] Log emitted storage deltas at debug level with address/key/value digests (hex).
- [ ] Add counters: number of metadata StorageChanges per contract type.

Rollout Strategy
- Default flags to false; validate locally.
- Enable in CI parity tests over sampled historical blocks.
- Gradually enable in larger replays; monitor mismatches and adjust.

Exit Criteria
- WitnessCreate/WitnessUpdate parity confirmed; first mismatch eliminated.
- VoteWitness initial parity (voter mapping digest) confirmed on sampled blocks.
- Sorting and digest stability validated across multiple runs.

Risks & Mitigations
- Cross-language serialization drift → Provide test vectors and unify specs; add golden tests.
- Tag/key collisions → Keep distinct tags; add unit tests for uniqueness.
- Performance (hashing) → Negligible vs block execution; batch compute where possible.


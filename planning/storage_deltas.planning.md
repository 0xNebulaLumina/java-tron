emit storage deltas (not a synthetic account change)

  Why

  - Fidelity to real state: WitnessCreate mutates the Witness store and account metadata, not the EVM account tuple (balance, nonce, code). Encoding that as a storage delta is semantically accurate and scales to WitnessUpdate/VoteWitness.
  - Extensibility: More system contracts mutate non-EVM stores. A principled “storage delta” channel covers all of them; a synthetic second AccountChange is a one-off hack that won’t generalize.
  - Determinism: With a deterministic keying/serialization for witness records, ordering and hashing stay stable across both embedded and remote.
  - Practicality: Today’s AccountChange payload (76 bytes) cannot encode flags. Adding a second AccountChange without a meaningful byte-delta will not match embedded digests. A storage delta can carry a faithful change signal without changing
  the 76-byte contract.

  How to align embedded vs remote

  - Short term (parity with minimal churn)
      - Emit a Witness store delta as a “metadata delta” alongside the owner AccountChange.
      - Use a deterministic key derived from the witness address and a 32-byte value derived from the witness record bytes to fit current types.
          - Key: keccak256(0x41 || owner_address || b":witness") → map to U256.
          - Value: keccak256(serialize(witness_info)) for new, and zero for old on create; on update, old = prev digest, new = new digest.
      - Ordering: emit owner AccountChange first, then the metadata StorageChange; stable sort by address then key (already implemented).
      - Do this on both paths:
          - Remote: add the delta right after put_witness(...) in rust-backend/crates/core/src/service.rs:499.
          - Embedded: instrument the same moment the Java Witness store is written to also emit the matching delta into the CSV recorder stream.
  - Medium term (richer metadata)
      - Continue emitting explicit store deltas for other governance/system stores (WitnessUpdate, VoteWitness tallies), with the same “digest-of-record” approach while you stabilize a canonical byte layout.
  - Long term (encode flags)
      - If you want flags inside the account payload, version the account-blob format (e.g., append a TLV after the 76 bytes with a version tag), and update both embedded and remote emitters.
      - Only roll this out once both sides write the same new format; preserve the old format via a feature flag to avoid breaking existing tools.

  Why not a second synthetic owner AccountChange

  - It won’t carry real metadata: the 76-byte blob has no place for flags. You’d end up duplicating the same bytes twice or smuggling flags into unrelated fields, which is brittle and misleading.
  - It won’t scale: VoteWitness and other contracts update multiple non-EVM structures. A synthetic account delta per structure becomes unmaintainable and still won’t encode the right semantics.

  Concrete next steps I can implement

  - Remote (Rust)
      - Add an optional metadata delta emission guarded by remote.execution.emit_storage_changes=true:
          - After put_witness(...), compute key_u256 = U256::from_be_bytes(keccak(0x41||addr||\":witness\")) and val_u256 = U256::from_be_bytes(keccak(serialize(witness))).
          - Push TronStateChange::StorageChange { address: owner, key: key_u256, old_value: U256::ZERO, new_value: val_u256 }.
          - File: rust-backend/crates/core/src/service.rs:436
  - Embedded (Java)
      - At witness creation/update points, emit the same “metadata delta” using the same derivation, so both CSVs converge:
          - Files: actuator/src/main/java/org/tron/core/actuator/WitnessCreateActuator.java:121; plus the CSV recorder hook you already introduced.
  - Validation
      - Re-run the CSV comparator for block 1785/tx 0 and confirm the digest parity.
      - Keep the feature flag so you can toggle while aligning other contracts.

  If you want me to proceed, I’ll:

  - Add the flagged metadata StorageChange in Rust as above.
  - Sketch the Java hook (location and digest derivation) so you can wire it into the embedded recorder.
  
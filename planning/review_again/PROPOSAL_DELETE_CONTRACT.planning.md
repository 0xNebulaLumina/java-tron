# PROPOSAL_DELETE_CONTRACT (18) — Rust vs Java parity review

## Scope
Review whether the Rust backend implementation of `PROPOSAL_DELETE_CONTRACT` matches java-tron’s behavior (validation + execution + persisted proposal bytes).

## References (source of truth)
- Java actuator: `actuator/src/main/java/org/tron/core/actuator/ProposalDeleteActuator.java`
- Java `Proposal` protobuf serialization (field order + map serialization call-site): `protocol/src/main/java/org/tron/protos/Protocol.java` (`Protocol.Proposal#writeTo`)
- Rust executor: `rust-backend/crates/core/src/service/mod.rs` (`execute_proposal_delete_contract`, `parse_proposal_delete_contract`)
- Rust proposal persistence encoder: `rust-backend/crates/execution/src/storage_adapter/engine.rs` (`encode_proposal_java_compatible`)
- Rust prost-build config affecting map type/order: `rust-backend/crates/execution/build.rs` (`config.btree_map([".protocol.Proposal.parameters", ...])`)

## What java-tron does (baseline)
`ProposalDeleteActuator.validate()` enforces, in order:
1. `Any` type must be `ProposalDeleteContract` (else: `contract type error,expected type [ProposalDeleteContract],real type[...]`).
2. `owner_address` must be a valid TRON address (`DecodeUtil.addressValid`) (else: `Invalid address`).
3. Owner account must exist (else: `Account[<hex>] not exists`).
4. `proposal_id` must be `<= LATEST_PROPOSAL_NUM` (else: `Proposal[<id>] not exists`).
5. Proposal must exist in `ProposalStore` (else: `Proposal[<id>] not exists`).
6. Proposal proposer must equal `owner_address` (else: `Proposal[<id>] is not proposed by <hex>`).
7. `now < expiration_time` (else: `Proposal[<id>] expired`).
8. Proposal state must not already be `CANCELED` (else: `Proposal[<id>] canceled`).

`ProposalDeleteActuator.execute()` then:
- Loads the proposal by id, sets `state = CANCELED`, and stores it back. Fee is `0`.

## What Rust does today
`execute_proposal_delete_contract` in `rust-backend/crates/core/src/service/mod.rs` mirrors the same checks and state transition:
- Checks `Any.type_url` matches `protocol.ProposalDeleteContract` (when `metadata.contract_parameter` is present).
- Parses `owner_address` + `proposal_id`.
- Validates address prefix/length and that the owner account exists.
- Checks `proposal_id <= latest_proposal_num` and that the proposal exists.
- Checks proposer matches owner, checks expiration and canceled status.
- Sets proposal `state = 3` and persists via `storage_adapter.put_proposal(&proposal)`.

For the conformance fixtures that exist today (happy path + typical validate_fail cases), this is *functionally* aligned with java-tron.

## Where it may NOT match java-tron (important edge cases)

### 1) `proposal_id` presence vs proto3 defaulting
Rust’s `parse_proposal_delete_contract` currently requires field `proposal_id` (2) to be present and errors with `Missing proposal_id` if absent.

In proto3/java-tron, an absent `proposal_id` field decodes as the default `0` (and validation then proceeds to fail later, typically with `Proposal[0] not exists`).

This means:
- A transaction whose `ProposalDeleteContract.proposal_id == 0` (commonly encoded with the field omitted) will produce a *different* validation error in Rust than in Java.

Even if this is “invalid anyway”, it breaks strict parity if error strings are compared.

### 2) Proposal `parameters` map ordering can be rewritten by Rust on delete
This is the bigger “think harder” mismatch risk.

Facts:
- Java persists proposals with `proposal.toByteArray()` (see `ProposalCapsule#getData`), and `Protocol.Proposal#writeTo` uses `GeneratedMessageV3.serializeLongMapTo(...)` for the `parameters` map field.
- Unless deterministic protobuf serialization is enabled on the `CodedOutputStream`, protobuf-java serializes maps in *map iteration order* (and protobuf’s `MapField` uses `LinkedHashMap`, so that order is effectively insertion order / parse order).
- Rust’s execution crate explicitly configures `.protocol.Proposal.parameters` as a `BTreeMap` in `rust-backend/crates/execution/build.rs`, and `encode_proposal_java_compatible` sorts entries by key.

Implication:
- If an existing stored proposal has multiple `parameters` entries in a non-sorted insertion order (possible depending on how the original `ProposalCreateContract` was constructed/serialized by clients), then **Rust’s ProposalDelete will re-serialize the proposal with `parameters` in sorted-by-key order**, even though java-tron would preserve the original insertion order when it only changes `state`.

Net effect:
- Semantics (“which parameters exist”) are unchanged, but the *persisted proposal bytes* can diverge from java-tron after a delete/cancel.
- Current `ProposalDelete` fixtures won’t catch this because their helper creates proposals with a single parameter (`params.put(0L, ...)`), so ordering is irrelevant.

### 3) Malformed protobuf bytes → error-string parity
Rust uses a lightweight protobuf parser for `ProposalDeleteContract`. For truncation/EOF cases, error strings may not match protobuf-java’s `InvalidProtocolBufferException` message unless explicitly normalized (as is done in some other parsers in `mod.rs`).

Not currently covered by fixtures, but it’s another potential parity gap if you ever add “invalid protobuf encoding” conformance cases.

## Bottom line
- **On “normal” transactions and the existing fixture set, Rust matches java-tron’s PROPOSAL_DELETE_CONTRACT logic.**
- **Strict parity is not guaranteed** for:
  - `proposal_id == 0` / omitted field (Rust errors early; Java defaults to 0 and errors later with different message),
  - proposals with multi-entry `parameters` maps that were stored with a non-sorted insertion order (Rust delete may reorder the map on re-encode).


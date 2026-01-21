# Review: `PROPOSAL_APPROVE_CONTRACT` parity (Rust backend vs java-tron)

## Scope

This review compares:

- **Rust backend**: `BackendService::execute_proposal_approve_contract()` + `parse_proposal_approve_contract()` in `rust-backend/crates/core/src/service/mod.rs`
- **Java reference**: `ProposalApproveActuator` + `ProposalCapsule` in:
  - `actuator/src/main/java/org/tron/core/actuator/ProposalApproveActuator.java`
  - `chainbase/src/main/java/org/tron/core/capsule/ProposalCapsule.java`

Goal: determine whether the Rust implementation matches java-tron’s **validation + state mutation** semantics for contract type 17.

---

## Java-side reference behavior (what “correct” means)

### 1) Validation (`ProposalApproveActuator.validate`)

Source: `actuator/src/main/java/org/tron/core/actuator/ProposalApproveActuator.java`

Order and conditions:

1. Contract / store existence checks (`any != null`, `chainBaseManager != null`)
2. Contract type check: `any.is(ProposalApproveContract.class)`
3. Parse `ProposalApproveContract`:
   - `bytes owner_address = 1`
   - `int64 proposal_id = 2`
   - `bool is_add_approval = 3`
4. Owner address validation: `DecodeUtil.addressValid(ownerAddress)`
   - non-empty
   - length == 21
   - prefix byte == `DecodeUtil.addressPreFixByte` (configured network prefix)
5. Account existence: `accountStore.has(ownerAddress)` else:
   - `Account[<hex ownerAddress>] not exists`
6. Witness existence: `witnessStore.has(ownerAddress)` else:
   - `Witness[<hex ownerAddress>] not exists`
7. Proposal existence (two-step):
   - if `proposal_id > dynamicStore.getLatestProposalNum()`:
     - `Proposal[<id>] not exists`
   - else load `proposalStore.get(proposal_id)`; if missing:
     - `Proposal[<id>] not exists`
8. Proposal liveness:
   - if `now >= proposal.expiration_time`:
     - `Proposal[<id>] expired`
   - if `proposal.state == CANCELED`:
     - `Proposal[<id>] canceled`
9. Approval add/remove semantics:
   - if removing (`is_add_approval == false`) and owner not in approvals:
     - `Witness[<hex owner>]has not approved proposal[<id>] before`
   - if adding (`is_add_approval == true`) and owner already in approvals:
     - `Witness[<hex owner>]has approved proposal[<id>] before`

### 2) Execution (`ProposalApproveActuator.execute`)

Source: `actuator/src/main/java/org/tron/core/actuator/ProposalApproveActuator.java`

- Loads proposal from `ProposalStore` by ID (throws if missing).
- Mutates approvals via `ProposalCapsule`:
  - add: `proposalCapsule.addApproval(owner)`
  - remove: `proposalCapsule.removeApproval(owner)`
- Persists updated proposal to `ProposalStore`.
- Fee is `0` (`calcFee()` returns `0`).

`ProposalCapsule.removeApproval` semantics (important edge detail):

- It copies approvals to a `List`, then calls `approvals.remove(address)` which removes **only the first matching entry**.
- Under normal operation duplicates should not exist (validate blocks repeat approvals), but if duplicates were present in DB, Java would remove only one.

---

## Rust backend behavior (what it currently does)

Source: `rust-backend/crates/core/src/service/mod.rs`

### Validation & parsing

- If `transaction.metadata.contract_parameter` is present:
  - checks Any type url matches `protocol.ProposalApproveContract`
  - returns Java-matching error string on mismatch
- Parses protobuf bytes with `parse_proposal_approve_contract()`:
  - field 1: owner_address (bytes)
  - field 2: proposal_id (varint)
  - field 3: is_add_approval (varint bool)
  - unknown fields are skipped by wire type
- Address validation:
  - `owner_address_bytes.len() == 21`
  - `owner_address_bytes[0] == storage_adapter.address_prefix()`
  - else returns `"Invalid address"`
- Account existence:
  - `get_account_proto(&owner_evm_20b).is_some()` else `Account[<hex owner>] not exists`
- Witness existence:
  - `is_witness(&owner_evm_20b)` else `Witness[<hex owner>] not exists`
- Proposal existence:
  - if `proposal_id > get_latest_proposal_num()` -> `Proposal[<id>] not exists`
  - else `get_proposal(proposal_id)` must be `Some`, else same error
- Liveness:
  - `now = get_latest_block_header_timestamp()`
  - `now >= proposal.expiration_time` -> `Proposal[<id>] expired`
  - `proposal.state == 3` -> `Proposal[<id>] canceled`
- Approval semantics:
  - matches Java error strings for already-approved / not-approved cases

### State changes / persistence

- add: `proposal.approvals.push(owner_address_bytes.clone())`
- remove: `proposal.approvals.retain(|a| a != &owner_address_bytes)`
- persists via `storage_adapter.put_proposal(&proposal)`

---

## Does it match java-tron?

### What matches (high confidence)

For all “normal” states exercised by the conformance fixtures, the Rust implementation is aligned with java-tron:

- **Validation ordering and checks** match `ProposalApproveActuator.validate` (address → account → witness → latestProposalNum → proposal fetch → expired/canceled → approval semantics).
- **Error message strings** match java-tron’s actuator strings (including the “`]has ...`” formatting).
- **State mutation** is equivalent when approvals are unique:
  - add appends one approval
  - remove deletes the approval
- **Proposal CANCELED check** uses numeric enum value `3`, which matches `protocol.Proposal.State.CANCELED = 3`.

### Where it may diverge (edge cases / long-term parity risks)

These are real semantic differences from Java, but likely not hit by current fixtures:

1) **Duplicate-approval removal semantics**
   - Java `removeApproval` removes only the first matching entry.
   - Rust `retain` removes **all** matching entries.
   - If a corrupted/non-canonical DB ever contains duplicate approvals, Java and Rust will produce different post-state.

2) **Unknown protobuf fields on `Proposal`**
   - Java protobuf preserves unknown fields when parsing + re-serializing a message via `toBuilder()...build()`.
   - `prost` decodes `Proposal` without retaining unknown fields, so Rust will drop unknown fields on any write (`put_proposal`).
   - If future versions add fields to `protocol.Proposal`, Java would round-trip them; Rust would erase them when approving/removing approvals.

3) **Contract-parameter presence / type checking**
   - Java always has `Any parameter` and validates `any.is(...)`.
   - Rust only performs the Any type-url check when `metadata.contract_parameter` is present (otherwise it parses `transaction.data` directly).
   - This is probably intentional for non-Java callers, but it’s not a byte-for-byte clone of Java’s “No contract!” path.

---

## Bottom line

- **Yes, the Rust implementation matches java-tron’s `ProposalApproveActuator` logic for canonical/expected state**, including all validations and the resulting proposal DB update semantics for unique approvals.
- **No, it is not a perfect behavioral clone in pathological/future-proof cases** (duplicate approvals, unknown-field round-tripping, and the “missing Any” path).


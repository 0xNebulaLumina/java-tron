Review Target

- Rust: `rust-backend/crates/core/src/service/mod.rs` (`execute_proposal_create_contract`)
- Rust storage: `rust-backend/crates/execution/src/storage_adapter/engine.rs` (proposal + dynamic-properties writes)
- Java reference: `actuator/src/main/java/org/tron/core/actuator/ProposalCreateActuator.java`
- Java parameter validation: `actuator/src/main/java/org/tron/core/utils/ProposalUtil.java` (`ProposalUtil.validator`)

Question

- Does the Rust implementation of `PROPOSAL_CREATE_CONTRACT` (type 16) really match the Java-side logic?

Answer (short)

- It matches the “skeleton” of Java’s `ProposalCreateActuator.validate()` and `execute()` plus a small subset of `ProposalUtil.validator` (the subset exercised by the current conformance fixtures).
- It does not match Java’s full parameter validation surface for all `ProposalUtil.ProposalType` codes (fork gating, per-code ranges, prerequisites, and “already active / no need to propose again” checks). In those cases, Rust will accept proposals that Java would reject.

What matches (confirmed parity)

1) Contract parsing / address validation

- Remote execution path passes raw `ProposalCreateContract` bytes in `transaction.data` (see Java `RemoteExecutionSPI`).
- Java: `DecodeUtil.addressValid(ownerAddress)` → error `"Invalid address"`.
- Rust: `owner_address_bytes.len() == 21 && owner_address_bytes[0] == storage_adapter.address_prefix()` → error `"Invalid address"`.

2) Owner existence + witness requirement

- Java:
  - `AccountStore.has(ownerAddress)` → `"Account[<hex>] not exists"`
  - `WitnessStore.has(ownerAddress)` → `"Witness[<hex>] not exists"`
- Rust:
  - `get_account_proto(&owner).is_some()` → `"Account[<hex>] not exists"`
  - `is_witness(&owner)` → `"Witness[<hex>] not exists"`
- Both use hex-encoded 21-byte TRON address in the message (Java `StringUtil.createReadableString` and Rust `hex::encode` align).

3) Empty parameter map check

- Java: `contract.getParametersMap().size() == 0` → `"This proposal has no parameter."`
- Rust: `parameters.is_empty()` → `"This proposal has no parameter."`

4) Parameter validation (subset)

Rust implements the following Java `ProposalUtil.validator(...)` cases with matching error messages:

- Code `0` (`MAINTENANCE_TIME_INTERVAL`): range `[3 * 27 * 1000, 24 * 3600 * 1000]`.
- Codes `1..=8` (fee-like params): range `[0, LONG_VALUE]` where `LONG_VALUE = 100_000_000_000_000_000`.
- Code `9` (`ALLOW_CREATION_OF_CONTRACTS`): must be `1`.
- Code `10` (`REMOVE_THE_POWER_OF_THE_GR`): must be `1` and must not already be executed (`getRemoveThePowerOfTheGr() != -1`).
- Code `18` (`ALLOW_TVM_TRANSFER_TRC10`): must be `1` and requires `ALLOW_SAME_TOKEN_NAME` already enabled.
- Unsupported code: `"Does not support code : X"` (matches Java `ProposalType.getEnum` throw).

5) Proposal id + time calculation + persistence

- Proposal ID:
  - Java: `latestProposalNum + 1`
  - Rust: `get_latest_proposal_num() + 1`
- Expiration time formula: Rust matches Java’s `now/nextMaintenance/maintenanceInterval/proposalExpireTime` calculation.
- Persistence:
  - Java: `ProposalStore.put(...)` + `DynamicPropertiesStore.saveLatestProposalNum(id)`
  - Rust: `put_proposal(...)` + `set_latest_proposal_num(id)`
- Initial proposal fields align: approvals empty; state defaults to `PENDING` (0).

What does NOT match (gaps vs Java)

1) Most of `ProposalUtil.validator` is not implemented

- Java validates every parameter entry via:
  - `ProposalUtil.validator(dynamicPropertiesStore, forkController, code, value)`
- Rust only special-cases codes: `0`, `1..=8`, `9`, `10`, `18`.
- For every other supported code, Rust only checks `is_supported_proposal_parameter_code(code)` and otherwise performs no per-code validation.

That means Rust currently accepts proposals that Java rejects. Examples:

- Code `13` (`MAX_CPU_TIME_OF_ONE_TX`):
  - Java enforces `[10,100]` or `[10,400]` depending on `ALLOW_HIGHER_LIMIT_FOR_MAX_CPU_TIME_OF_ONE_TX`.
  - Rust has no range check.
- Codes `14/15/16` (`ALLOW_UPDATE_ACCOUNT_NAME`, `ALLOW_SAME_TOKEN_NAME`, `ALLOW_DELEGATE_RESOURCE`):
  - Java requires `value == 1`.
  - Rust accepts `0` or other values.
- Fork-gated codes (many):
  - Java rejects with `BAD_PARAM_ID` / `"Bad chain parameter id [...]"` when `forkController.pass(...)` is false.
  - Rust has no fork gating at all (no equivalent of Java’s `ForkController`).
- Prerequisite / dependency checks:
  - Java has many “must be approved before X can be proposed” guards (e.g., `ALLOW_TVM_CONSTANTINOPLE`, `ALLOW_TVM_SOLIDITY_059`, `FORBID_TRANSFER_TO_CONTRACT`, `ALLOW_TVM_FREEZE`, `ALLOW_CANCEL_ALL_UNFREEZE_V2`, `MAX_DELEGATE_LOCK_PERIOD`, `ALLOW_OLD_REWARD_OPT`, etc.).
  - Rust only enforces the single prerequisite for code `18` (TRC10 transfer requires `ALLOW_SAME_TOKEN_NAME`).
- “Already active / no need to propose again” checks:
  - Java blocks re-proposing already-enabled toggles for some params (e.g., `ALLOW_NEW_REWARD`, `ALLOW_OLD_REWARD_OPT`, `ALLOW_STRICT_MATH`, `CONSENSUS_LOGIC_OPTIMIZATION`, `ALLOW_TVM_CANCUN`, `ALLOW_TVM_BLOB`, etc.).
  - Rust does not implement these checks.

2) Optional Any/type-url validation parity

- Java `validate()` rejects wrong `Any` type (`any.is(ProposalCreateContract.class)`).
- Rust `execute_proposal_create_contract` always parses `transaction.data` and does not check `transaction.metadata.contract_parameter.type_url` even if present (approve/delete handlers do a best-effort type-url check).

3) Write-model / integration nuance (depends on deployment)

- Rust persists proposal + `LATEST_PROPOSAL_NUM` directly and returns `state_changes = []`.
- Java’s architecture comments elsewhere in the repo describe a “compute-only + Java apply” mode for avoiding double-writes.
- If proposal execution is ever used in a mode where Java is still the authoritative persistence path (embedded DB), Rust would need a way to return ProposalStore/DynamicProperties mutations as sidecars/state-changes (or signal `write_mode=PERSISTED` and ensure Java does not apply).

Why this can look “correct” today

- The current conformance fixtures for `proposal_create_contract` mainly cover the subset Rust implemented:
  - address/account/witness checks, empty params
  - code `0` bounds
  - codes `1..=8` negative/out-of-range
  - code `9` must be 1
  - code `10` one-time + must be 1
  - code `18` prerequisite + must be 1
  - unsupported code
- So Rust can pass conformance while still being incomplete vs full Java logic.

Bottom line

- If the goal is consensus-grade Java parity, Rust `PROPOSAL_CREATE_CONTRACT` needs a near-1:1 port of `ProposalUtil.validator` (including fork gating + prerequisites + “already active” guards) and optional `Any` type checking.
- If the goal is only to cover the current fixture subset, the implementation matches that subset, but it should be treated as intentionally partial and kept behind a tight feature gate.

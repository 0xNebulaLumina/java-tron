# NON_VM `contract_parameter` Rollout and Parser Reuse Plan

## Audit result

The two source statements are not both correct as written.

- Statement 1 is only partially true.
  - It is true that the original "apply strict `contract_parameter` handling to all NON_VM handlers" rollout is not finished.
  - It is not true that strict handling exists only for ClearABI.
  - Current Rust already has strict presence/type handling in a small subset:
    - Shared helper path: `AccountPermissionUpdate`, `UpdateSetting`, `UpdateEnergyLimit`, `ClearABI`, `UpdateBrokerage`
    - Manual strict presence path: `ProposalApprove`, `WithdrawExpireUnfreeze`, `CancelAllUnfreezeV2`
- Statement 2 is false.
  - The protobuf error taxonomy in `rust-backend/crates/core/src/service/contracts/proto.rs` is already reused outside ClearABI.
  - Current reuse exists in:
    - `parse_account_update_contract`
    - `parse_account_permission_update_contract`
    - `parse_update_setting_contract`
    - `parse_update_energy_limit_contract`
    - `parse_update_brokerage_contract`
  - There is still real remaining work, but it is "extend reuse further", not "start reuse from zero".

## Important parity correction

Do not copy the old ClearABI improvement note verbatim.

- Java distinguishes:
  - missing `Any` (`this.any == null`) -> usually `"No contract!"`
  - present `Any` with wrong or empty `type_url` -> type mismatch message
- Current Rust helper `require_contract_parameter(...)` collapses missing `contract_parameter` into the type-mismatch path for helper-based handlers.
- If we finish the rollout, we should fix that semantic gap instead of spreading it to more handlers.

## Goal

Finish the original rollout cleanly:

- every Rust NON_VM handler enforces Java-like `contract_parameter` presence and type checks
- handlers that unpack contract bytes stop silently falling back to `transaction.data`
- remaining handwritten protobuf parsers migrate onto the shared typed error utilities where feasible
- tests explicitly lock in null-vs-type-mismatch behavior and malformed-protobuf behavior

## Current coverage snapshot

### Already strict, but helper semantics need improvement

- `AccountPermissionUpdate`
- `UpdateSetting`
- `UpdateEnergyLimit`
- `ClearABI`
- `UpdateBrokerage`

Issue:

- these use the shared helper today
- missing `contract_parameter` currently returns the type-mismatch string
- Java would first fail on missing `Any`

### Already strict by manual check

- `ProposalApprove`
- `WithdrawExpireUnfreeze`
- `CancelAllUnfreezeV2`

Issue:

- these preserve the missing-parameter path better than the helper-based contracts
- they should still be normalized onto shared helper logic so null/type/type-url behavior stays consistent

### Optional check or fallback still present

- `Transfer`
- `WitnessCreate`
- `WitnessUpdate`
- `AccountUpdate`
- `AccountCreate`
- `ProposalCreate`
- `ProposalDelete`
- `SetAccountId`
- `TransferAsset`
- `AssetIssue`
- `UnfreezeAsset`
- `UpdateAsset`
- `FreezeBalance`
- `UnfreezeBalance`
- `FreezeBalanceV2`
- `UnfreezeBalanceV2`
- `WithdrawBalance`
- `MarketCancelOrder`
- `MarketSellAsset`

Issue:

- these still accept missing `contract_parameter`, or only validate it when present, or still parse from `transaction.data`

### No meaningful `contract_parameter` parity yet

- `VoteWitness`
- `DelegateResource`
- `UndelegateResource`
- `ParticipateAssetIssue`
- `ExchangeCreate`
- `ExchangeInject`
- `ExchangeWithdraw`
- `ExchangeTransaction`

Issue:

- these do not yet enforce the Java-side `Any` contract invariant at the handler boundary

## Parser reuse snapshot

### Already using the shared typed protobuf helpers

- `contracts/proto.rs::parse_account_update_contract`
- `mod.rs::parse_account_permission_update_contract`
- `mod.rs::parse_update_setting_contract`
- `mod.rs::parse_update_energy_limit_contract`
- `mod.rs::parse_update_brokerage_contract`

### Partially aligned, but still ad hoc

- `mod.rs::parse_clear_abi_contract`
- `mod.rs::parse_proposal_delete_contract`

### Good candidates for the next migration wave

- `parse_vote_witness_contract`
- `validate_witness_update_contract_bytes`
- `parse_account_create_contract`
- `parse_proposal_create_contract`
- freeze/unfreeze V1 and V2 parsers in `contracts/freeze.rs`
- `parse_delegate_resource_contract`
- `parse_undelegate_resource_contract`
- `parse_participate_asset_issue_contract`
- `parse_unfreeze_asset_owner_address`
- `parse_update_asset_contract`
- exchange parsers
- market parsers

## Proposed implementation plan

### Phase 1: Fix the shared contract-parameter helper API

Files:

- `rust-backend/crates/core/src/service/mod.rs`

Change:

- replace the current single helper with two explicit layers

Recommended shape:

- `require_contract_any(transaction, missing_error) -> Result<&Any, String>`
- `require_contract_type(any, expected_type, type_error) -> Result<&[u8], String>`
- optional convenience wrapper:
  - `require_contract_parameter_value(transaction, expected_type, missing_error, type_error)`

Why:

- preserves Java ordering
- lets each handler keep its exact missing-Any error string
- avoids baking the wrong "missing == type mismatch" rule into the rest of the rollout

Acceptance criteria:

- helper-based contracts can return the Java-appropriate missing error
- empty `type_url` is treated as a present-but-wrong Any, not as missing

### Phase 2: Finish the strict NON_VM rollout contract family by contract family

#### 2A. Normalize the already-strict handlers

Contracts:

- `AccountPermissionUpdate`
- `UpdateSetting`
- `UpdateEnergyLimit`
- `ClearABI`
- `UpdateBrokerage`
- `ProposalApprove`
- `WithdrawExpireUnfreeze`
- `CancelAllUnfreezeV2`

Work:

- move them onto the new helper shape
- preserve exact Java missing/type-mismatch ordering
- keep `contract_parameter.value` as the canonical protobuf source

#### 2B. Convert the optional/fallback handlers

Contracts:

- `Transfer`
- `WitnessCreate`
- `WitnessUpdate`
- `AccountUpdate`
- `AccountCreate`
- `ProposalCreate`
- `ProposalDelete`
- `SetAccountId`
- `TransferAsset`
- `AssetIssue`
- `UnfreezeAsset`
- `UpdateAsset`
- `FreezeBalance`
- `UnfreezeBalance`
- `FreezeBalanceV2`
- `UnfreezeBalanceV2`
- `WithdrawBalance`
- `MarketCancelOrder`
- `MarketSellAsset`

Work pattern for each:

- require `contract_parameter`
- validate expected `type_url`
- remove "if present" logic
- stop using `transaction.data` as a fallback when protobuf bytes are needed
- preserve existing business validation order after the Any gate

Notes:

- some handlers may still execute primarily from split transaction fields (`from`, `to`, `value`)
- even for those handlers, the Any presence/type gate should still happen first if Java validates through `any`

#### 2C. Add Any parity to the handlers that currently ignore it

Contracts:

- `VoteWitness`
- `DelegateResource`
- `UndelegateResource`
- `ParticipateAssetIssue`
- `ExchangeCreate`
- `ExchangeInject`
- `ExchangeWithdraw`
- `ExchangeTransaction`

Work:

- add the same missing/type gate
- switch contract parsing to `contract_parameter.value` when the Java actuator effectively does `any.unpack(...)`

### Phase 3: Extend shared protobuf error taxonomy reuse

Files:

- `rust-backend/crates/core/src/service/contracts/proto.rs`
- `rust-backend/crates/core/src/service/mod.rs`
- `rust-backend/crates/core/src/service/contracts/freeze.rs`
- `rust-backend/crates/core/src/service/contracts/withdraw.rs`

Approach:

- migrate remaining handwritten protobuf parsers from:
  - `read_varint(...)`
  - stringly error mapping
  - unchecked/legacy skip helpers
- onto:
  - `read_tag_typed(...)`
  - `read_length_delimited_typed(...)`
  - `skip_protobuf_field_checked(...)`
  - `ProtobufError::to_java_message()`

Priority order:

- parsers already involved in contract-parameter rollout
- parsers with existing malformed-protobuf tests
- parsers that still leak custom strings such as `Failed to read field header: ...`

Acceptance criteria:

- malformed protobuf failures use shared typed classification rather than ad hoc string mapping
- legacy `skip_protobuf_field(...)` usage keeps shrinking instead of spreading

### Phase 4: Tests

Rust tests:

- add per-contract tests for:
  - missing `contract_parameter`
  - wrong `type_url`
  - empty `type_url`
  - malformed `contract_parameter.value`
  - `contract_parameter.value` taking precedence over `transaction.data`

Conformance tests:

- keep using Java-generated malformed protobuf fixtures where Java can produce them
- do not rely on conformance fixtures for missing `contract_parameter`, because the Java fixture path always sets it
- cover missing-Any behavior in Rust unit tests instead

Regression scope:

- `cargo test --package tron-backend-core service::tests`
- focused suites for each converted contract family
- existing conformance suites that exercise malformed protobuf handling

## Recommended execution order

1. Refactor helper API first.
2. Fix the currently strict helper-based contracts so missing-Any behavior becomes correct.
3. Convert the optional/fallback contracts in the account/proposal/metadata family.
4. Convert the resource family in `freeze.rs` and `withdraw.rs`.
5. Convert TRC-10, market, exchange, and delegation contracts.
6. Sweep remaining handwritten parsers onto `contracts/proto.rs` helpers.
7. Run targeted Rust tests after each family, then a final full focused sweep.

## Definition of done

- every NON_VM handler rejects missing `contract_parameter` before normal field validation
- wrong or empty `type_url` returns the contract-specific type error, not a fallback parse error
- handlers no longer silently substitute `transaction.data` for missing `contract_parameter.value`
- shared protobuf error taxonomy is used by the remaining handwritten parsers that currently still map errors ad hoc
- tests cover the null/type/protobuf-failure matrix for each converted family

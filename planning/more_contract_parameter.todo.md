# NON_VM `contract_parameter` Rollout Checklist

Status: complete
Owner: Rust backend NON_VM execution path

## Audit and policy

- [x] Record the corrected audit result in code review / implementation notes:
- [x] Statement "strict handling only exists for ClearABI" is false as written.
- [x] Statement "protobuf error taxonomy has not been reused elsewhere" is false.
- [x] Confirm Java null-vs-type-mismatch semantics for each contract family before changing helpers.
- [x] Stop carrying forward the old assumption that empty `type_url` should be treated as "missing".

## Shared helper refactor

- [x] Replace `require_contract_parameter(...)` with helper(s) that distinguish:
- [x] missing `contract_parameter`
- [x] present `contract_parameter` with wrong or empty `type_url`
- [x] canonical access to `contract_parameter.value`
- [x] Keep contract-specific missing/type-mismatch strings configurable per handler.
- [x] Update existing helper call sites to the new API.

## Fix already-strict handlers

- [x] `AccountPermissionUpdate`: preserve Java missing-Any error path.
- [x] `UpdateSetting`: preserve Java missing-Any error path.
- [x] `UpdateEnergyLimit`: preserve Java missing-Any error path.
- [x] `ClearABI`: preserve Java missing-Any error path.
- [x] `UpdateBrokerage`: preserve Java missing-Any error path.
- [x] `ProposalApprove`: normalize onto shared helper without changing behavior.
- [x] `WithdrawExpireUnfreeze`: normalize onto shared helper without changing behavior.
- [x] `CancelAllUnfreezeV2`: normalize onto shared helper without changing behavior.

## Convert optional/fallback handlers

- [x] `Transfer`: require `contract_parameter` before business validation.
- [x] `WitnessCreate`: require `contract_parameter` before business validation.
- [x] `WitnessUpdate`: require `contract_parameter` before business validation.
- [x] `AccountUpdate`: require `contract_parameter`; keep `contract_parameter.value` as canonical bytes.
- [x] `AccountCreate`: require `contract_parameter`; remove `transaction.data` fallback.
- [x] `ProposalCreate`: require `contract_parameter`; remove `transaction.data` fallback.
- [x] `ProposalDelete`: require `contract_parameter`; remove `transaction.data` fallback.
- [x] `SetAccountId`: require `contract_parameter`; remove `transaction.data` fallback.
- [x] `TransferAsset`: require `contract_parameter`; keep Any as source of truth where contract bytes are needed.
- [x] `AssetIssue`: require `contract_parameter`; remove `transaction.data` fallback.
- [x] `UnfreezeAsset`: require `contract_parameter`; remove `transaction.data` fallback.
- [x] `UpdateAsset`: require `contract_parameter`; remove `transaction.data` fallback.
- [x] `FreezeBalance`: require `contract_parameter`; parse from Any bytes.
- [x] `UnfreezeBalance`: require `contract_parameter`; parse from Any bytes.
- [x] `FreezeBalanceV2`: require `contract_parameter`; parse from Any bytes.
- [x] `UnfreezeBalanceV2`: require `contract_parameter`; parse from Any bytes.
- [x] `WithdrawBalance`: require `contract_parameter` before business validation.
- [x] `MarketCancelOrder`: require `contract_parameter`; remove `transaction.data` fallback.
- [x] `MarketSellAsset`: require `contract_parameter`; remove `transaction.data` fallback.

## Add Any parity to handlers that currently ignore it

- [x] `VoteWitness`: add missing/type gate and parse from Any bytes.
- [x] `DelegateResource`: add missing/type gate and parse from Any bytes.
- [x] `UndelegateResource`: add missing/type gate and parse from Any bytes.
- [x] `ParticipateAssetIssue`: add missing/type gate and parse from Any bytes.
- [x] `ExchangeCreate`: add missing/type gate and parse from Any bytes.
- [x] `ExchangeInject`: add missing/type gate and parse from Any bytes.
- [x] `ExchangeWithdraw`: add missing/type gate and parse from Any bytes.
- [x] `ExchangeTransaction`: add missing/type gate and parse from Any bytes.

## Parser taxonomy reuse

- [x] Keep `contracts/proto.rs` as the shared home for typed protobuf helpers.
- [x] Migrate `parse_vote_witness_contract` to typed helpers.
- [x] Migrate `validate_witness_update_contract_bytes` to typed helpers where feasible.
- [x] Migrate `parse_account_create_contract` to typed helpers.
- [x] Migrate `parse_proposal_create_contract` to typed helpers.
- [x] Migrate freeze/unfreeze V1 parsers in `contracts/freeze.rs` to typed helpers.
- [x] Migrate freeze/unfreeze V2 parsers in `contracts/freeze.rs` to typed helpers.
- [x] Migrate `parse_delegate_resource_contract` to typed helpers.
- [x] Migrate `parse_undelegate_resource_contract` to typed helpers.
- [x] Migrate `parse_participate_asset_issue_contract` to typed helpers.
- [x] Migrate `parse_unfreeze_asset_owner_address` to typed helpers.
- [x] Migrate `parse_update_asset_contract` to typed helpers.
- [x] Migrate exchange parsers to typed helpers.
- [x] Migrate market parsers to typed helpers.
- [x] Reduce remaining uses of legacy `read_varint(...)` for contract protobuf decoding.
- [x] Reduce remaining uses of legacy `skip_protobuf_field(...)` for contract protobuf decoding.

## Tests

- [x] Add Rust tests for missing `contract_parameter` on each converted family.
- [x] Add Rust tests for wrong `type_url` on each converted family.
- [x] Add Rust tests for empty `type_url` on each converted family.
- [x] Add Rust tests for malformed `contract_parameter.value` on each converted family.
- [x] Add Rust tests that `contract_parameter.value` wins over `transaction.data` where both exist.
- [x] Keep existing conformance fixture coverage for malformed protobuf categories.
- [x] Add new Java fixtures only where Java can naturally produce the malformed Any payload under test.
- [x] Do not depend on Java conformance fixtures for missing-Any coverage.

## Verification

- [x] Run focused Rust tests for helper-based contracts after the helper refactor.
- [x] Run focused Rust tests for account/proposal family after conversion.
- [x] Run focused Rust tests for resource family after conversion.
- [x] Run focused Rust tests for TRC-10 / market / exchange / delegation families after conversion.
- [x] Run conformance-focused Rust tests for malformed protobuf behavior.
- [x] Review for accidental behavior changes in existing error strings.
- [x] Review for any remaining `transaction.data` fallback on NON_VM handlers that unpack contract bytes.

## Done when

- [x] Every NON_VM Rust handler enforces Java-like `contract_parameter` presence and type checks.
- [x] Missing `contract_parameter` no longer falls through to unrelated validation or parsing.
- [x] Wrong or empty `type_url` fails as a type mismatch, not as "missing".
- [x] Remaining handwritten protobuf parsers use the shared typed error utilities wherever practical.
- [x] Test coverage locks the null/type/malformed-protobuf matrix across the converted handlers.

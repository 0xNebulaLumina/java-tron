# NON_VM `contract_parameter` Rollout Checklist

Status: planning only
Owner: Rust backend NON_VM execution path

## Audit and policy

- [ ] Record the corrected audit result in code review / implementation notes:
- [ ] Statement "strict handling only exists for ClearABI" is false as written.
- [ ] Statement "protobuf error taxonomy has not been reused elsewhere" is false.
- [ ] Confirm Java null-vs-type-mismatch semantics for each contract family before changing helpers.
- [ ] Stop carrying forward the old assumption that empty `type_url` should be treated as "missing".

## Shared helper refactor

- [ ] Replace `require_contract_parameter(...)` with helper(s) that distinguish:
- [ ] missing `contract_parameter`
- [ ] present `contract_parameter` with wrong or empty `type_url`
- [ ] canonical access to `contract_parameter.value`
- [ ] Keep contract-specific missing/type-mismatch strings configurable per handler.
- [ ] Update existing helper call sites to the new API.

## Fix already-strict handlers

- [ ] `AccountPermissionUpdate`: preserve Java missing-Any error path.
- [ ] `UpdateSetting`: preserve Java missing-Any error path.
- [ ] `UpdateEnergyLimit`: preserve Java missing-Any error path.
- [ ] `ClearABI`: preserve Java missing-Any error path.
- [ ] `UpdateBrokerage`: preserve Java missing-Any error path.
- [ ] `ProposalApprove`: normalize onto shared helper without changing behavior.
- [ ] `WithdrawExpireUnfreeze`: normalize onto shared helper without changing behavior.
- [ ] `CancelAllUnfreezeV2`: normalize onto shared helper without changing behavior.

## Convert optional/fallback handlers

- [ ] `Transfer`: require `contract_parameter` before business validation.
- [ ] `WitnessCreate`: require `contract_parameter` before business validation.
- [ ] `WitnessUpdate`: require `contract_parameter` before business validation.
- [ ] `AccountUpdate`: require `contract_parameter`; keep `contract_parameter.value` as canonical bytes.
- [ ] `AccountCreate`: require `contract_parameter`; remove `transaction.data` fallback.
- [ ] `ProposalCreate`: require `contract_parameter`; remove `transaction.data` fallback.
- [ ] `ProposalDelete`: require `contract_parameter`; remove `transaction.data` fallback.
- [ ] `SetAccountId`: require `contract_parameter`; remove `transaction.data` fallback.
- [ ] `TransferAsset`: require `contract_parameter`; keep Any as source of truth where contract bytes are needed.
- [ ] `AssetIssue`: require `contract_parameter`; remove `transaction.data` fallback.
- [ ] `UnfreezeAsset`: require `contract_parameter`; remove `transaction.data` fallback.
- [ ] `UpdateAsset`: require `contract_parameter`; remove `transaction.data` fallback.
- [ ] `FreezeBalance`: require `contract_parameter`; parse from Any bytes.
- [ ] `UnfreezeBalance`: require `contract_parameter`; parse from Any bytes.
- [ ] `FreezeBalanceV2`: require `contract_parameter`; parse from Any bytes.
- [ ] `UnfreezeBalanceV2`: require `contract_parameter`; parse from Any bytes.
- [ ] `WithdrawBalance`: require `contract_parameter` before business validation.
- [ ] `MarketCancelOrder`: require `contract_parameter`; remove `transaction.data` fallback.
- [ ] `MarketSellAsset`: require `contract_parameter`; remove `transaction.data` fallback.

## Add Any parity to handlers that currently ignore it

- [ ] `VoteWitness`: add missing/type gate and parse from Any bytes.
- [ ] `DelegateResource`: add missing/type gate and parse from Any bytes.
- [ ] `UndelegateResource`: add missing/type gate and parse from Any bytes.
- [ ] `ParticipateAssetIssue`: add missing/type gate and parse from Any bytes.
- [ ] `ExchangeCreate`: add missing/type gate and parse from Any bytes.
- [ ] `ExchangeInject`: add missing/type gate and parse from Any bytes.
- [ ] `ExchangeWithdraw`: add missing/type gate and parse from Any bytes.
- [ ] `ExchangeTransaction`: add missing/type gate and parse from Any bytes.

## Parser taxonomy reuse

- [ ] Keep `contracts/proto.rs` as the shared home for typed protobuf helpers.
- [ ] Migrate `parse_vote_witness_contract` to typed helpers.
- [ ] Migrate `validate_witness_update_contract_bytes` to typed helpers where feasible.
- [ ] Migrate `parse_account_create_contract` to typed helpers.
- [ ] Migrate `parse_proposal_create_contract` to typed helpers.
- [ ] Migrate freeze/unfreeze V1 parsers in `contracts/freeze.rs` to typed helpers.
- [ ] Migrate freeze/unfreeze V2 parsers in `contracts/freeze.rs` to typed helpers.
- [ ] Migrate `parse_delegate_resource_contract` to typed helpers.
- [ ] Migrate `parse_undelegate_resource_contract` to typed helpers.
- [ ] Migrate `parse_participate_asset_issue_contract` to typed helpers.
- [ ] Migrate `parse_unfreeze_asset_owner_address` to typed helpers.
- [ ] Migrate `parse_update_asset_contract` to typed helpers.
- [ ] Migrate exchange parsers to typed helpers.
- [ ] Migrate market parsers to typed helpers.
- [ ] Reduce remaining uses of legacy `read_varint(...)` for contract protobuf decoding.
- [ ] Reduce remaining uses of legacy `skip_protobuf_field(...)` for contract protobuf decoding.

## Tests

- [ ] Add Rust tests for missing `contract_parameter` on each converted family.
- [ ] Add Rust tests for wrong `type_url` on each converted family.
- [ ] Add Rust tests for empty `type_url` on each converted family.
- [ ] Add Rust tests for malformed `contract_parameter.value` on each converted family.
- [ ] Add Rust tests that `contract_parameter.value` wins over `transaction.data` where both exist.
- [ ] Keep existing conformance fixture coverage for malformed protobuf categories.
- [ ] Add new Java fixtures only where Java can naturally produce the malformed Any payload under test.
- [ ] Do not depend on Java conformance fixtures for missing-Any coverage.

## Verification

- [ ] Run focused Rust tests for helper-based contracts after the helper refactor.
- [ ] Run focused Rust tests for account/proposal family after conversion.
- [ ] Run focused Rust tests for resource family after conversion.
- [ ] Run focused Rust tests for TRC-10 / market / exchange / delegation families after conversion.
- [ ] Run conformance-focused Rust tests for malformed protobuf behavior.
- [ ] Review for accidental behavior changes in existing error strings.
- [ ] Review for any remaining `transaction.data` fallback on NON_VM handlers that unpack contract bytes.

## Done when

- [ ] Every NON_VM Rust handler enforces Java-like `contract_parameter` presence and type checks.
- [ ] Missing `contract_parameter` no longer falls through to unrelated validation or parsing.
- [ ] Wrong or empty `type_url` fails as a type mismatch, not as "missing".
- [ ] Remaining handwritten protobuf parsers use the shared typed error utilities wherever practical.
- [ ] Test coverage locks the null/type/malformed-protobuf matrix across the converted handlers.

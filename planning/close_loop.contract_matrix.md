# Close Loop — 1.5 Contract Support Matrix

This file closes Section 1.5 of `close_loop.todo.md`. It enumerates every
contract type that the Phase 1 `RR` path currently tries to execute, tags
each with a readiness classification, and names a **Phase 1 canonical
whitelist** — the contracts we are going to stake EE-vs-RR parity on.

This is the living readiness matrix referenced in Section 6.5. Update it
whenever a contract changes classification. Do not let the matrix drift
from the code.

## Classification tags

| Tag                  | Meaning                                                                 |
| -------------------- | ----------------------------------------------------------------------- |
| `EE only`            | Never in scope for Phase 1 `RR`. Stay on the Java embedded path.        |
| `RR blocked`         | Rust code exists, but a known semantic hole prevents Phase 1 acceptance. |
| `RR candidate`       | Rust code exists and is being validated, but not yet stable enough to stake EE-vs-RR parity on. |
| `RR canonical-ready` | Passes all Phase 1 readiness requirements: fixture coverage, Rust unit coverage, EE-vs-RR replay coverage, and the known-gap list is empty. Eligible for the Phase 1 whitelist target. **At time of writing no contract holds this tag yet** — the EE-vs-RR replay pipeline has not produced data on any contract. The tag is reserved for contracts that pass that bar in the future. |

Two facts about a contract are easy to confuse and need to stay
separated in the matrix:

- **Implemented** — does Rust have a handler at all?
- **Default enabled** — does the checked-in `config.toml` and the
  `RemoteExecutionConfig::default()` in `config.rs` ship with the
  contract's `*_enabled` flag set to `true`?

A contract can be implemented without being default-enabled, in which
case it only runs when a test or operator turns the flag on. The
"Default enabled" column below records that explicitly.

## Readiness attributes

Each contract records seven attributes:

- **Read-path closure** — does the contract's Rust handler depend on a
  placeholder `getCode` / `getStorageAt` / `getBalance` / etc. that we
  still need to close?
- **TRC-10 semantics** — does the contract depend on TRC-10 ledger
  persistence being real across sidecars?
- **Freeze/resource sidecars** — does it depend on `FreezeLedgerChange`,
  `GlobalResourceTotalsChange`, etc., being applied correctly?
- **Dynamic-property strictness** — does it depend on
  `strict_dynamic_properties` being enabled and matched on both sides?
- **Fixture coverage** — is there at least one Java-side conformance
  fixture that exercises the contract end-to-end?
- **Rust unit coverage** — are there Rust unit tests for the handler's
  validation + state mutation logic?
- **EE-vs-RR replay coverage** — has any automated EE-vs-RR replay
  path actually compared outputs on this contract type?

Attribute values: `yes`, `no`, `n/a`, `tbd` (needs audit).

## Matrix

### System contracts (classification + enablement)

Columns:

- **Implemented** — Rust handler exists (`yes` / `no`).
- **Java gate** — `RemoteExecutionSPI` reachability for the contract.
  `always` = no special gate beyond the contract type switch.
  `flag` = there is a gating Java system property (e.g.
  `-Dremote.exec.trc10.enabled=true`) that must be set or the Java
  bridge throws `UnsupportedOperationException`.
- **Rust default** — value of the `*_enabled` flag in the
  checked-in `rust-backend/config.toml` (or `(code default)` if the
  flag is not present in `config.toml` and falls back to the
  `RemoteExecutionConfig::default()` value in `config.rs`, which is
  almost always `false` for non-baseline flags).
- **Tag** — readiness classification (see above).

| Contract                           | Config flag                          | Implemented | Java gate                                    | Rust default                  | Tag            |
| ---------------------------------- | ------------------------------------ | ----------- | -------------------------------------------- | ----------------------------- | -------------- |
| TransferContract                   | `system_enabled`                     | yes         | always                                        | `system_enabled = true`       | RR candidate   |
| CreateSmartContract                | (VM path)                            | yes         | always                                        | (VM path; no per-contract flag) | RR candidate |
| TriggerSmartContract               | (VM path)                            | partial     | always (Rust handler explicit-rejects TRC-10 pre-transfer) | (VM path)         | **RR blocked** |
| WitnessCreateContract              | `witness_create_enabled`             | yes         | always                                        | `witness_create_enabled = true` | RR candidate |
| WitnessUpdateContract              | `witness_update_enabled`             | yes         | always                                        | `witness_update_enabled = true` | RR candidate |
| VoteWitnessContract                | `vote_witness_enabled`               | yes         | always                                        | `vote_witness_enabled = true` | RR candidate |
| AccountUpdateContract              | (always on via system_enabled)       | yes         | always                                        | (rides on `system_enabled`)   | RR candidate |
| AccountCreateContract              | `account_create_enabled`             | yes         | always                                        | `account_create_enabled = true` | RR candidate |
| FreezeBalanceContract              | `freeze_balance_enabled`             | yes         | always                                        | `freeze_balance_enabled = true` | RR candidate |
| UnfreezeBalanceContract            | `unfreeze_balance_enabled`           | yes         | always                                        | `unfreeze_balance_enabled = true` | RR candidate |
| FreezeBalanceV2Contract            | `freeze_balance_v2_enabled`          | yes         | always                                        | `freeze_balance_v2_enabled = true` | RR candidate |
| UnfreezeBalanceV2Contract          | `unfreeze_balance_v2_enabled`        | yes         | always                                        | `unfreeze_balance_v2_enabled = true` | RR candidate |
| WithdrawBalanceContract            | `withdraw_balance_enabled`           | yes         | always                                        | `withdraw_balance_enabled = true` | RR candidate |
| AssetIssueContract                 | `trc10_enabled`                      | yes         | flag (`-Dremote.exec.trc10.enabled=true`)     | `trc10_enabled = true`        | RR candidate   |
| TransferAssetContract              | `trc10_enabled`                      | yes         | flag (`-Dremote.exec.trc10.enabled=true`)     | `trc10_enabled = true`        | RR candidate   |
| ParticipateAssetIssueContract      | `participate_asset_issue_enabled`    | yes         | flag (`-Dremote.exec.trc10.enabled=true`)     | `participate_asset_issue_enabled = true` | RR candidate |
| UnfreezeAssetContract              | `unfreeze_asset_enabled`             | yes         | flag (`-Dremote.exec.trc10.enabled=true`)     | `unfreeze_asset_enabled = true` | RR candidate |
| UpdateAssetContract                | `update_asset_enabled`               | yes         | flag (`-Dremote.exec.trc10.enabled=true`)     | `update_asset_enabled = true` | RR candidate |
| ProposalCreateContract             | `proposal_create_enabled`            | yes         | flag (`-Dremote.exec.proposal.enabled=true`)  | `(code default)` = `false`    | RR candidate   |
| ProposalApproveContract            | `proposal_approve_enabled`           | yes         | flag (`-Dremote.exec.proposal.enabled=true`)  | `(code default)` = `false`    | RR candidate   |
| ProposalDeleteContract             | `proposal_delete_enabled`            | yes         | flag (`-Dremote.exec.proposal.enabled=true`)  | `(code default)` = `false`    | RR candidate   |
| SetAccountIdContract               | `set_account_id_enabled`             | yes         | flag (`-Dremote.exec.account.enabled=true`)   | `(code default)` = `false`    | RR candidate   |
| AccountPermissionUpdateContract    | `account_permission_update_enabled`  | yes         | flag (`-Dremote.exec.account.enabled=true`)   | `(code default)` = `false`    | RR candidate   |
| UpdateSettingContract              | `update_setting_enabled`             | yes         | flag (`-Dremote.exec.contract.enabled=true`)  | `(code default)` = `false`    | RR candidate   |
| UpdateEnergyLimitContract          | `update_energy_limit_enabled`        | yes         | flag (`-Dremote.exec.contract.enabled=true`)  | `(code default)` = `false`    | RR candidate   |
| ClearABIContract                   | `clear_abi_enabled`                  | yes         | flag (`-Dremote.exec.contract.enabled=true`)  | `(code default)` = `false`    | RR candidate   |
| UpdateBrokerageContract            | `update_brokerage_enabled`           | yes         | flag (`-Dremote.exec.brokerage.enabled=true`) | `(code default)` = `false`    | RR candidate   |
| WithdrawExpireUnfreezeContract     | `withdraw_expire_unfreeze_enabled`   | yes         | flag (`-Dremote.exec.resource.enabled=true`)  | `(code default)` = `false`    | RR candidate   |
| DelegateResourceContract           | `delegate_resource_enabled`          | yes         | flag (`-Dremote.exec.resource.enabled=true`)  | `(code default)` = `false`    | RR candidate   |
| UnDelegateResourceContract         | `undelegate_resource_enabled`        | yes         | flag (`-Dremote.exec.resource.enabled=true`)  | `(code default)` = `false`    | RR candidate   |
| CancelAllUnfreezeV2Contract        | `cancel_all_unfreeze_v2_enabled`     | yes         | flag (`-Dremote.exec.resource.enabled=true`)  | `(code default)` = `false`    | RR candidate   |
| ExchangeCreateContract             | `exchange_create_enabled`            | yes         | flag (`-Dremote.exec.exchange.enabled=true`)  | `(code default)` = `false`    | RR candidate   |
| ExchangeInjectContract             | `exchange_inject_enabled`            | yes         | flag (`-Dremote.exec.exchange.enabled=true`)  | `(code default)` = `false`    | RR candidate   |
| ExchangeWithdrawContract           | `exchange_withdraw_enabled`          | yes         | flag (`-Dremote.exec.exchange.enabled=true`)  | `(code default)` = `false`    | RR candidate   |
| ExchangeTransactionContract        | `exchange_transaction_enabled`       | yes         | flag (`-Dremote.exec.exchange.enabled=true`)  | `(code default)` = `false`    | RR candidate   |
| MarketSellAssetContract            | `market_sell_asset_enabled`          | yes         | flag (`-Dremote.exec.market.enabled=true`)    | `(code default)` = `false`    | RR candidate   |
| MarketCancelOrderContract          | `market_cancel_order_enabled`        | yes         | flag (`-Dremote.exec.market.enabled=true`)    | `(code default)` = `false`    | RR candidate   |
| ShieldedTransferContract           | (not mapped)                         | no          | n/a                                           | n/a                           | EE only        |

Important corrections from the previous draft of this matrix:

- No contract is currently `RR canonical-ready`. Earlier drafts of
  this file tagged `TransferContract`, `CreateSmartContract`, and
  `UpdateSettingContract` as `RR canonical-ready` based on baseline
  unit tests passing, but the canonical-ready definition requires
  EE-vs-RR replay coverage and that pipeline has not produced data
  yet. Those three are now `RR candidate` and named below as the
  Phase 1 **whitelist target** — the contracts we are driving toward
  canonical-ready, not the contracts that already are.
- Many `RR candidate` flags are NOT enabled in the checked-in
  `rust-backend/config.toml`. Roughly: the witness/freeze/withdraw/
  account/TRC-10 families ship enabled, but the proposal, account
  permission, contract-metadata, resource-delegation, exchange, and
  market families ship with the code default of `false`. Tests must
  override the flag (or `config.toml` must add it) before the Rust
  handler is reached.

Notes:

- `TriggerSmartContract` is tagged **RR blocked** because of the TRC-10
  pre-execution token transfer gap tracked in
  `planning/review_again/TRIGGER_SMART_CONTRACT.todo.md` and in
  Section 5.1 of the main todo. Until that gap is closed with explicit
  parity semantics, Trigger cannot be on the Phase 1 whitelist target.
  The current explicit reject path in the Rust handler must stay
  until the replacement semantics are designed.
- `ShieldedTransferContract` is not mapped in `RemoteExecutionSPI` at
  all and is explicitly not on the Rust side's roadmap. It is `EE only`.
- The set of contracts whose `*_enabled` flag is `true` in
  `rust-backend/config.toml` is a **subset** of the `RR candidate`
  list, not the whole list. See the per-row "Rust default" column
  above for which families are off-by-default. A contract being
  `RR candidate` only says "Rust has a handler"; whether the handler
  will actually be reached at runtime depends on the Java gate and
  the Rust default.

### Detailed readiness attributes (to audit)

| Contract                      | Reads | TRC-10 | Freeze/res sidecar | Dyn-prop strict | Fixture cov | Rust unit cov | EE-vs-RR replay |
| ----------------------------- | ----- | ------ | ------------------- | --------------- | ----------- | ------------- | --------------- |
| TransferContract              | n/a   | no     | no                  | no              | yes         | yes           | tbd             |
| CreateSmartContract           | yes   | no     | no                  | yes             | yes         | yes           | tbd             |
| UpdateSettingContract         | yes   | no     | no                  | no              | yes         | yes           | tbd             |
| WitnessCreateContract         | no    | no     | no                  | yes             | tbd         | yes           | tbd             |
| WitnessUpdateContract         | no    | no     | no                  | no              | tbd         | yes           | tbd             |
| VoteWitnessContract           | no    | no     | yes                 | yes             | tbd         | yes           | tbd             |
| AccountUpdateContract         | no    | no     | no                  | no              | tbd         | yes           | tbd             |
| AccountCreateContract         | no    | no     | no                  | yes             | tbd         | yes           | tbd             |
| FreezeBalanceContract         | no    | no     | **yes**             | yes             | tbd         | yes           | tbd             |
| UnfreezeBalanceContract       | no    | no     | **yes**             | yes             | tbd         | yes           | tbd             |
| FreezeBalanceV2Contract       | no    | no     | **yes**             | yes             | tbd         | yes           | tbd             |
| UnfreezeBalanceV2Contract     | no    | no     | **yes**             | yes             | tbd         | yes           | tbd             |
| WithdrawBalanceContract       | no    | no     | yes                 | yes             | tbd         | yes           | tbd             |
| ProposalCreateContract        | no    | no     | no                  | yes             | tbd         | tbd           | tbd             |
| ProposalApproveContract       | no    | no     | no                  | yes             | tbd         | tbd           | tbd             |
| ProposalDeleteContract        | no    | no     | no                  | yes             | tbd         | tbd           | tbd             |
| SetAccountIdContract          | no    | no     | no                  | no              | tbd         | tbd           | tbd             |
| AccountPermissionUpdateContract | no  | no     | no                  | yes             | tbd         | tbd           | tbd             |
| UpdateEnergyLimitContract     | yes   | no     | no                  | yes             | tbd         | tbd           | tbd             |
| ClearABIContract              | yes   | no     | no                  | yes             | tbd         | tbd           | tbd             |
| UpdateBrokerageContract       | no    | no     | no                  | yes             | tbd         | tbd           | tbd             |
| WithdrawExpireUnfreezeContract | no   | no     | **yes**             | yes             | tbd         | tbd           | tbd             |
| DelegateResourceContract      | no    | no     | **yes**             | yes             | tbd         | tbd           | tbd             |
| UnDelegateResourceContract    | no    | no     | **yes**             | yes             | tbd         | tbd           | tbd             |
| CancelAllUnfreezeV2Contract   | no    | no     | **yes**             | yes             | tbd         | tbd           | tbd             |
| AssetIssueContract            | no    | **yes**| no                  | yes             | yes         | yes           | tbd             |
| TransferAssetContract         | no    | **yes**| no                  | yes             | tbd         | tbd           | tbd             |
| ParticipateAssetIssueContract | no    | **yes**| no                  | yes             | tbd         | tbd           | tbd             |
| UnfreezeAssetContract         | no    | **yes**| no                  | yes             | tbd         | tbd           | tbd             |
| UpdateAssetContract           | no    | **yes**| no                  | yes             | tbd         | tbd           | tbd             |
| ExchangeCreateContract        | no    | **yes**| no                  | yes             | tbd         | tbd           | tbd             |
| ExchangeInjectContract        | no    | **yes**| no                  | yes             | tbd         | tbd           | tbd             |
| ExchangeWithdrawContract      | no    | **yes**| no                  | yes             | tbd         | tbd           | tbd             |
| ExchangeTransactionContract   | no    | **yes**| no                  | yes             | tbd         | tbd           | tbd             |
| MarketSellAssetContract       | no    | **yes**| no                  | yes             | tbd         | tbd           | tbd             |
| MarketCancelOrderContract     | no    | **yes**| no                  | yes             | tbd         | tbd           | tbd             |
| TriggerSmartContract          | yes   | **yes**| no                  | yes             | yes         | yes           | blocked         |
| ShieldedTransferContract      | —     | —      | —                   | —               | —           | —             | n/a (EE only)   |

`tbd` values are genuine audit gaps — the readiness dashboard (Section
6.5) is where we turn them into `yes`/`no` once someone has actually
inspected the relevant code and tests.

## Phase 1 whitelist target (first cut)

This is the **target** list of contracts we are driving toward
`RR canonical-ready` during Phase 1. None of these are canonical-ready
yet — the EE-vs-RR replay pipeline must produce green results on each
before its tag changes from `RR candidate` to `RR canonical-ready`.

1. `TransferContract`
2. `CreateSmartContract`
3. `UpdateSettingContract`

For each contract on this target list, the Phase 1 work is:

- Confirm the contract's Java gate is reachable in the test harness
  (for `UpdateSettingContract`, this means turning on
  `update_setting_enabled` on both sides during the parity run; the
  flag is **not** in the checked-in `rust-backend/config.toml`).
- Add or confirm a deterministic EE-vs-RR replay test that runs the
  contract under both backends and diffs output state and receipts.
- Resolve every `tbd` attribute in the readiness rows below until the
  row is fully populated and there are no open gaps.
- Only then re-tag the contract from `RR candidate` to
  `RR canonical-ready` in the matrix above.

Once **all three** target contracts have been re-tagged
canonical-ready, the "a first contract whitelist reaches stable
EE-vs-RR parity" exit criterion in Section 0 can be checked.

Rationale for the first cut:

- `TransferContract` is simple, well-tested on both sides, and does not
  depend on any of the Phase 1 open workstreams. It is the smoke-test.
- `CreateSmartContract` exercises the VM write-path end to end and is
  already reported passing in the close_loop baseline signals
  (`cargo test -p tron-backend-core create_smart_contract`). It is the
  VM reference contract.
- `UpdateSettingContract` exercises `ContractStore` reads, `AccountStore`
  owner validation, and dynamic-property gates. It is the "reads +
  writes + dynamic prop" reference contract. Its baseline test is
  already reported passing (`cargo test -p tron-backend-core
  update_setting`).

Explicitly **not on the first cut**, with rationale:

- `TriggerSmartContract` — blocked by the TRC-10 pre-execution transfer
  gap (Section 5.1).
- All TRC-10 family contracts — depend on TRC-10 ledger parity across
  sidecars, which is not yet closed.
- All freeze/unfreeze family contracts — depend on sidecar parity
  (FreezeLedgerChange, GlobalResourceTotalsChange) which is still being
  shaken out in Section 5.2.
- Proposal / permission / exchange / market families — lower priority
  than VM and simple system contracts, deferred until the first cut
  is green.

The whitelist grows as tbd audits turn green. Each additional contract
is a separate roadmap item — expanding the whitelist is intentional,
not automatic.

## Secondary list

These stay `RR candidate` through Phase 1 and will be re-evaluated at
Phase 1 handoff. They are **not** acceptance blockers, but they may be
enabled for opportunistic parity runs if the Phase 1 whitelist target is
green:

- WitnessCreate, WitnessUpdate, VoteWitness
- Freeze family (V1 + V2), WithdrawBalance, WithdrawExpireUnfreeze,
  Delegate/UnDelegate, CancelAllUnfreezeV2
- Account family (Create, Update, PermissionUpdate, SetAccountId)
- Proposal family (Create, Approve, Delete)
- Update family (EnergyLimit, ClearABI, UpdateBrokerage)
- TRC-10 family (AssetIssue, TransferAsset, ParticipateAssetIssue,
  UnfreezeAsset, UpdateAsset)
- Exchange family (Create, Inject, Withdraw, Transaction)
- Market family (SellAsset, CancelOrder)

## Blocked list

Contracts that cannot progress past `RR blocked` until their dependency
is closed:

- `TriggerSmartContract` — blocked on Section 5.1 (TRC-10 pre-execution
  token transfer design). The existing explicit reject path in the Rust
  handler stays until replacement semantics are agreed.

## EE-only list

Contracts that are intentionally not routed through `RR`:

- `ShieldedTransferContract`

## Using the matrix

- Any code change that adds a new contract type MUST add a row here
  and choose a tag.
- Any code change that closes a `tbd` attribute (e.g., adds fixture
  coverage) MUST flip the attribute in this file in the same commit.
- Any change to `rust-backend/config.toml` that enables a contract
  flag MUST check the matrix first: if the contract is still
  `RR blocked`, the flag change is a regression and should be
  reverted.
- The "Phase 1 whitelist target" above is advisory until Section 6.5
  wires it into an actual dashboard. Once that dashboard exists,
  this file becomes the source-of-truth for what it displays.

## Follow-up implementation items

These are tracked against Section 6.5 and Section 2 / 3 work:

- [ ] Resolve every `tbd` attribute by auditing the relevant code
      and tests, starting with the Phase 1 whitelist target.
- [ ] Wire this matrix into an actual dashboard (Section 6.5) so it
      stops being a human-maintained markdown table.
- [ ] Close the Phase 1 whitelist target: drive all three
      whitelist contracts to stable EE-vs-RR replay parity.
- [ ] Re-tag `TriggerSmartContract` from `RR blocked` once Section
      5.1 is closed.
- [ ] Re-tag freeze/unfreeze family contracts once Section 5.2
      sidecar parity is closed.

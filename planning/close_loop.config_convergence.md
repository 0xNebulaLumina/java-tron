# Close Loop — 5.3 Config Flag Convergence

This file closes Section 5.3 of `close_loop.todo.md`. It audits every
`execution.remote.*` flag currently present in the checked-in
`rust-backend/config.toml` and the `RemoteExecutionConfig::default()`
implementation in `rust-backend/crates/common/src/config.rs`, then
documents two recommended profiles.

Companion notes:

- `close_loop.scope.md` — Phase 1 strategic modes and non-goals.
- `close_loop.contract_matrix.md` — per-contract readiness (Section 1.5).
- `close_loop.write_ownership.md` — `rust_persist_enabled` policy.
- `close_loop.energy_limit.md` — energy_limit wire contract.

## Audit table

Columns:

- **Flag** — the `execution.remote.*` setting name.
- **config.toml** — the value in the checked-in `rust-backend/config.toml`,
  or `(absent)` if the file does not set it (in which case the
  `RemoteExecutionConfig::default()` value applies).
- **code default** — the value in `RemoteExecutionConfig::default()`
  (`rust-backend/crates/common/src/config.rs`).
- **classification** — one of:
  - `EE baseline only` — must always be on for both `EE` and `RR` to function.
  - `RR experimental` — Rust handler exists, validation in progress, NOT
    on the Phase 1 whitelist target. Off-by-default unless explicitly enabled.
  - `RR canonical-ready` — would only apply once a contract reaches
    canonical-ready in `close_loop.contract_matrix.md`. **No flag is
    canonical-ready today** — see Section 1.5.
  - `legacy / removable` — kept for backward compatibility but should
    be deleted in a follow-up cleanup.

| Flag                                       | config.toml | code default | classification |
| ------------------------------------------ | ----------- | ------------ | -------------- |
| `system_enabled`                           | `true`      | `true`       | EE baseline only |
| `rust_persist_enabled`                     | `true`      | `false`      | RR experimental (canonical RR profile sets `true`; see write_ownership.md) |
| `witness_create_enabled`                   | `true`      | `true`       | RR experimental |
| `witness_update_enabled`                   | `true`      | `true`       | RR experimental |
| `vote_witness_enabled`                     | `true`      | `false`      | RR experimental |
| `vote_witness_seed_old_from_account`       | `true`      | `true`       | EE baseline only (matches embedded semantics) |
| `trc10_enabled`                            | `true`      | `false`      | RR experimental |
| `participate_asset_issue_enabled`          | `true`      | `false`      | RR experimental |
| `unfreeze_asset_enabled`                   | `true`      | `false`      | RR experimental |
| `update_asset_enabled`                     | `true`      | `false`      | RR experimental |
| `freeze_balance_enabled`                   | `true`      | `false`      | RR experimental |
| `unfreeze_balance_enabled`                 | `true`      | `false`      | RR experimental |
| `freeze_balance_v2_enabled`                | `true`      | `false`      | RR experimental |
| `unfreeze_balance_v2_enabled`              | `true`      | `false`      | RR experimental |
| `withdraw_balance_enabled`                 | `true`      | `false`      | RR experimental |
| `account_create_enabled`                   | `true`      | `false`      | RR experimental |
| `emit_freeze_ledger_changes`               | `true`      | `false`      | RR experimental (sidecar parity in 5.2) |
| `emit_global_resource_changes`             | `true`      | `false`      | RR experimental (sidecar parity in 5.2) |
| `emit_storage_changes`                     | `false`     | `false`      | RR experimental |
| `accountinfo_aext_mode`                    | `"hybrid"`  | `"none"`     | RR experimental (CSV parity for AEXT — set to `"hybrid"` for Phase 1 parity runs) |
| `delegation_reward_enabled`                | `true`      | `false`      | **legacy / removable** (the field is documented as deprecated in `config.rs`; delegation reward is now always computed when the dynamic property is enabled) |
| `proposal_create_enabled`                  | (absent)    | `false`      | RR experimental |
| `proposal_approve_enabled`                 | (absent)    | `false`      | RR experimental |
| `proposal_delete_enabled`                  | (absent)    | `false`      | RR experimental |
| `proposal_expire_time_ms`                  | (absent)    | `259200000`  | EE baseline only (matches CommonParameter default) |
| `set_account_id_enabled`                   | (absent)    | `false`      | RR experimental |
| `account_permission_update_enabled`        | (absent)    | `false`      | RR experimental |
| `update_setting_enabled`                   | (absent)    | `false`      | RR experimental (whitelist target — needs explicit enable for parity runs) |
| `update_energy_limit_enabled`              | (absent)    | `false`      | RR experimental |
| `clear_abi_enabled`                        | (absent)    | `false`      | RR experimental |
| `update_brokerage_enabled`                 | (absent)    | `false`      | RR experimental |
| `withdraw_expire_unfreeze_enabled`         | (absent)    | `false`      | RR experimental |
| `delegate_resource_enabled`                | (absent)    | `false`      | RR experimental |
| `undelegate_resource_enabled`              | (absent)    | `false`      | RR experimental |
| `cancel_all_unfreeze_v2_enabled`           | (absent)    | `false`      | RR experimental |
| `exchange_create_enabled`                  | (absent)    | `false`      | RR experimental |
| `exchange_inject_enabled`                  | (absent)    | `false`      | RR experimental |
| `exchange_withdraw_enabled`                | (absent)    | `false`      | RR experimental |
| `exchange_transaction_enabled`             | (absent)    | `false`      | RR experimental |
| `market_sell_asset_enabled`                | (absent)    | `false`      | RR experimental |
| `market_cancel_order_enabled`              | (absent)    | `false`      | RR experimental |
| `market_strict_index_parity`               | `true`      | `false`      | EE baseline only (Java parity for cancel order missing-state errors) |
| `strict_dynamic_properties`                | `true`      | `false`      | EE baseline only (Java parity for missing dynamic property keys) |
| `genesis_block_timestamp`                  | `1529891469000` | `1529891469000` | EE baseline only |
| `genesis_guard_representatives_base58`     | (absent)    | `[]`         | EE baseline only (empty = use hardcoded mainnet/testnet fallback) |

Notes:

- The current checked-in `config.toml` is much closer to the
  experimental profile below than to the conservative profile. This
  has been a source of "stable by config file, experimental by code
  comment" confusion in earlier planning rounds; the two profiles
  below replace that with explicit naming.
- Per Section 1.1's decision, `rust_persist_enabled = true` is the
  canonical `RR` writer profile, even though the *code default* is
  `false`. The classification above reflects that nuance.
- Several Java-side gates (`-Dremote.exec.proposal.enabled=true`,
  `-Dremote.exec.account.enabled=true`,
  `-Dremote.exec.contract.enabled=true`,
  `-Dremote.exec.brokerage.enabled=true`,
  `-Dremote.exec.resource.enabled=true`,
  `-Dremote.exec.exchange.enabled=true`,
  `-Dremote.exec.market.enabled=true`,
  `-Dremote.exec.trc10.enabled=true`) must also be set on the Java
  process to actually exercise the corresponding Rust path. The
  `close_loop.contract_matrix.md` "Java gate" column records which
  contract families need which JVM property.

## Recommended profile A — conservative (`EE` baseline only)

Goal: a deterministic configuration where only the `EE` execution path
is exercised. Useful as a control baseline for any EE-vs-RR comparison.

```
# rust-backend/config.toml (conservative)
[execution.remote]
system_enabled = true                 # required for the gRPC server to come up
rust_persist_enabled = false          # Rust does not write to its own RocksDB

# Everything else stays at its code default (false), so no per-contract
# flag is set in this profile.

# Strict parity helpers (kept on for clean errors, off-by-effect when no
# contract executes through the Rust path):
strict_dynamic_properties = true
market_strict_index_parity = true
```

Java side:

```
-Dexecution.mode=EMBEDDED
-Dstorage.mode=EMBEDDED
```

In this profile the `RR` path is effectively dormant — the only thing
the Rust backend does is bind its gRPC port. EE-vs-RR comparison runs
that need a baseline use this profile for the EE side.

## Recommended profile B — experimental (`RR` parity work)

Goal: the configuration we use to drive EE-vs-RR parity runs while we
are still in `RR experimental` territory. This is the profile the
current checked-in `rust-backend/config.toml` aims for.

```
# rust-backend/config.toml (experimental — current checked-in shape)
[execution.remote]
system_enabled = true
rust_persist_enabled = true                  # canonical RR writer

# Witness/vote families
witness_create_enabled = true
witness_update_enabled = true
vote_witness_enabled = true
vote_witness_seed_old_from_account = true

# TRC-10 family
trc10_enabled = true
participate_asset_issue_enabled = true
unfreeze_asset_enabled = true
update_asset_enabled = true

# Freeze / unfreeze families
freeze_balance_enabled = true
unfreeze_balance_enabled = true
freeze_balance_v2_enabled = true
unfreeze_balance_v2_enabled = true

# Resource / withdraw / account families
withdraw_balance_enabled = true
account_create_enabled = true

# Sidecar emission for sidecar-parity work (Section 5.2)
emit_freeze_ledger_changes = true
emit_global_resource_changes = true
emit_storage_changes = false                 # leave off until contract-matrix attribute resolved

# AEXT presence parity
accountinfo_aext_mode = "hybrid"

# Java parity strictness
strict_dynamic_properties = true
market_strict_index_parity = true
```

Java side:

```
-Dexecution.mode=REMOTE
-Dstorage.mode=REMOTE                                # aspirational; see write_ownership.md
-Dremote.exec.trc10.enabled=true
# (Add the family JVM gates below only when actively driving that family
#  to canonical-ready. Each adds another contract subset to the active
#  RR path, which means more variables to debug if parity breaks.)
# -Dremote.exec.proposal.enabled=true
# -Dremote.exec.account.enabled=true
# -Dremote.exec.contract.enabled=true
# -Dremote.exec.brokerage.enabled=true
# -Dremote.exec.resource.enabled=true
# -Dremote.exec.exchange.enabled=true
# -Dremote.exec.market.enabled=true
```

This profile is **explicitly experimental**. Results from a run under
this profile cannot be cited as "RR canonical-ready" until the
relevant contract has reached `RR canonical-ready` in
`close_loop.contract_matrix.md`.

## Decision

- Adopt profile B (current shape) as the Phase 1 parity-work profile
  and document it in this file. Keep the existing `config.toml`
  unchanged on disk — its current contents are the experimental
  profile.
- Adopt profile A as the conservative comparison baseline. It does
  not need its own checked-in config file; engineers running a
  conservative comparison should set the values explicitly per run.
- Mark `delegation_reward_enabled` as legacy / removable. A follow-up
  task in this section deletes the field; for this iteration we
  only flag it.
- Mark every other flag in the `RR experimental` row as off-the-Phase-1-
  acceptance-path until the corresponding contract reaches
  canonical-ready. Section 6.5 (contract readiness dashboard) is the
  feedback loop that drives flips.

## Definition of "no longer 'stable by config file, experimental by code comment'"

The accepted state is:

- `config.toml` clearly identifies itself as the experimental profile.
- The conservative profile is documented in this file but not
  duplicated as a separate checked-in file.
- The `delegation_reward_enabled` field is tagged for removal.
- The `close_loop.contract_matrix.md` "Default enabled" column
  matches the audit table above (already done in iteration 1's
  matrix work).

Section 5.3 acceptance is satisfied once the above are true and this
file is referenced from the relevant code/config comments. The
iteration-2 follow-ups below track the small comment edits needed.

## Follow-up implementation items

- [x] Add a header comment to `rust-backend/config.toml` that says
      "this file is the Phase 1 experimental profile; see
      `planning/close_loop.config_convergence.md` for the conservative
      profile and the per-flag classification".
      (Done in iter 2 — see top of `rust-backend/config.toml`.)
- [x] Add a corresponding header comment in
      `RemoteExecutionConfig` (`config.rs`) pointing at this file.
      (Done in iter 2 — see the doc comment on
      `RemoteExecutionConfig` in `crates/common/src/config.rs`.)
- [ ] Delete the deprecated `delegation_reward_enabled` field in a
      follow-up cleanup PR (and remove the corresponding row from
      `config.toml`). Tracked under Section 5.3 cleanup.
- [ ] When a contract reaches `RR canonical-ready` in
      `close_loop.contract_matrix.md`, flip its row above from
      "RR experimental" to "RR canonical-ready" in the same change.

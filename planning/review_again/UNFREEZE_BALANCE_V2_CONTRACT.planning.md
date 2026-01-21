# Review: `UNFREEZE_BALANCE_V2_CONTRACT` parity (Rust backend vs java-tron)

## Conclusion (short)
The Rust implementation in `rust-backend/crates/core/src/service/contracts/freeze.rs` is *mostly* a faithful port of java-tron’s `UnfreezeBalanceV2Actuator` for validation ordering, account proto mutations (`frozenV2`/`unfrozenV2`/`balance`), global weight deltas, and the vote/`oldTronPower` transition logic.

That said, there are a few **real parity gaps** where Rust can diverge from Java’s persisted state and/or “remote reporting” outputs.

## What Java does (reference behavior)
Primary reference: `actuator/src/main/java/org/tron/core/actuator/UnfreezeBalanceV2Actuator.java`.

### Validation (`validate()`)
Java rejects unless:
- `DynamicPropertiesStore.supportUnfreezeDelay()` is enabled (committee gate).
- `DecodeUtil.addressValid(ownerAddress)` succeeds.
- Owner account exists.
- Resource enum is valid:
  - `BANDWIDTH`/`ENERGY` always allowed
  - `TRON_POWER` allowed only when `ALLOW_NEW_RESOURCE_MODEL` is enabled
  - unknown enum value rejected with resource-specific error message.
- A positive `unfreeze_balance` is provided and `unfreeze_balance <= frozenAmount` for that resource.
- `accountCapsule.getUnfreezingV2Count(now) < UNFREEZE_MAX_TIMES` (32).

### Execution (`execute()`)
High-level effects:
1) Calls `MortgageService.withdrawReward(owner)` (delegation reward accounting; updates allowance + delegation store cycles).
2) Sweeps expired `unfrozenV2` entries into balance (`unfreezeExpire()`); emits `withdraw_expire_amount` in receipt.
3) If `ALLOW_NEW_RESOURCE_MODEL` and `oldTronPower == 0`, initializes `oldTronPower` from `getTronPower()` (0 → -1).
4) Appends a new `UnFreezeV2{type, amount, expireTime=now+UNFREEZE_DELAY_DAYS*FROZEN_PERIOD}` entry.
5) Updates:
   - owner’s `frozenV2` amount for that resource (subtract),
   - `DynamicPropertiesStore` total weights (delta of `/ TRX_PRECISION`).
6) Updates votes (`updateVote()`):
   - **New model transition**: if `ALLOW_NEW_RESOURCE_MODEL` and `oldTronPower != -1`, clears votes and returns.
   - **New model steady state**: if `ALLOW_NEW_RESOURCE_MODEL` and `oldTronPower == -1` and resource is `BANDWIDTH`/`ENERGY`, returns immediately (no rescale).
   - **Legacy model path**: rescales votes if owned tron power becomes insufficient.
7) If `ALLOW_NEW_RESOURCE_MODEL` and `oldTronPower != -1`, invalidates `oldTronPower` (sets to -1).

## What Rust does (current implementation)
Primary reference: `rust-backend/crates/core/src/service/contracts/freeze.rs::execute_unfreeze_balance_v2_contract`.

Rust mirrors the Java “core” behavior:
- Same committee gate (`support_unfreeze_delay()`).
- Address validation against a chain prefix (via `transaction.metadata.from_raw`).
- Same resource enum validation + error messages.
- Same range validation for `unfreeze_balance`.
- Same unfreezing-times limit logic (counts `unfrozen_v2` entries with `expire_time > now` and enforces `< 32`).
- Same sweep semantics (`expire_time <= now` gets withdrawn into balance, removed from `unfrozen_v2`).
- Same `unfreeze_expire_time` computation (`UNFREEZE_DELAY_DAYS * 86_400_000`).
- Same `frozenV2` decrease + keep-entry-even-when-0 behavior.
- Same global weight delta logic (use `(frozenV2 + delegatedV2) / TRX_PRECISION` before/after).
- Same `old_tron_power` initialization/invalidation pattern.
- Attempts to match vote migration and rescaling semantics by reading/writing the Votes store.

## Known / likely mismatches vs Java

### 1) Missing `MortgageService.withdrawReward(owner)` side-effect
Java calls it at the beginning of `execute()`; Rust currently does not.
- Java reference: `chainbase/src/main/java/org/tron/core/service/MortgageService.java::withdrawReward`.
- Rust has a port (`rust-backend/crates/core/src/service/contracts/delegation.rs::withdraw_reward`) but it is not wired into `execute_unfreeze_balance_v2_contract`.

Impact:
- Affects delegation reward bookkeeping and allowance updates.
- Can change later behavior of `WITHDRAW_BALANCE_CONTRACT` and reward-related invariants.

### 2) Vote update early-return under the new resource model
Java `updateVote()` returns immediately when:
- `ALLOW_NEW_RESOURCE_MODEL` is enabled,
- `oldTronPower == -1` (invalid),
- and resource is `BANDWIDTH`/`ENERGY`.

Rust’s implementation has the same branch, but **does not return**; it can fall through to the “rescale votes if insufficient tron power” block.

Why this matters:
- Under the new model with `oldTronPower == -1`, Java’s `getAllTronPower()` excludes bandwidth/energy contributions, so unfreezing bandwidth/energy should not impact vote adequacy.
- Falling through can cause Rust to rescale votes even when Java would have left them unchanged.

### 3) V2 “no expiration” semantics in Rust-side freeze ledger reporting
This contract updates an internal Rust-only `freeze-records` DB and emits `FreezeLedgerChange` when enabled.

Java treats V2 freeze/unfreeze as **no-expiration** (expiration = 0) at the account-state level, and any “freeze ledger reporting” should keep `expiration_ms == 0` for `v2_model=true`.

Rust currently:
- Preserves an existing `freeze-record` expiration when updating the record during V2 unfreeze.
- Uses that stored expiration in the emitted `FreezeLedgerChange` for `v2_model=true`.

Impact:
- Does not change the canonical Java account proto state (`frozenV2` has no expiration field), but can break CSV/domain-journal parity and “remote reporting” outputs.
- This also couples `UNFREEZE_BALANCE_V2_CONTRACT` correctness to how `FREEZE_BALANCE_V2_CONTRACT` seeds `freeze-records` expiration.

### 4) Empty-parameter edge case
Rust rejects empty `transaction.data` with `UnfreezeBalanceV2 params cannot be empty`.
Java would decode empty bytes to a default proto and then fail later (typically `Invalid address`).

Impact:
- Mostly affects malformed inputs / fixtures, but it is a real message-ordering difference.

### 5) Owner address source-of-truth
Rust validates the owner address via `transaction.metadata.from_raw` (not by decoding field 1 of the protobuf).
This matches how the conformance runner plumbs fixtures into `TxMetadata`, but it means:
- standalone unit tests can be misleading if they omit `from_raw`, and
- if both `from_raw` and the proto owner field are present and disagree, Rust does not currently cross-check them.

## Bottom line
- **Consensus-critical account proto changes** (frozenV2/unfrozenV2/balance, total weights, and the main vote mutation paths) look intentionally aligned with java-tron.
- **Not 1:1 parity** today due to: missing withdrawReward, a vote early-return difference under the new resource model, and V2 expiration handling in Rust-only reporting.

See `planning/review_again/UNFREEZE_BALANCE_V2_CONTRACT.todo.md` for a concrete fix checklist.


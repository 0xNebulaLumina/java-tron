# TODO / Fix Plan: `WITNESS_CREATE_CONTRACT` parity gaps

This checklist assumes we want Rust remote execution to match java-tron's `WitnessCreateActuator` behavior, including the multi-sign permission side effects.

## 0) Confirm the parity target (do this first)

- [x] Confirm the target environment's dynamic properties (mainnet-like vs fixtures):
  - [x] `ALLOW_MULTI_SIGN` (expected `1`)
  - [x] `ACTIVE_DEFAULT_OPERATIONS` value (32 bytes, default `7fff1fc0033e...`)
  - [x] `ALLOW_BLACKHOLE_OPTIMIZATION` behavior
  - [x] `ACCOUNT_UPGRADE_COST` value (default 9999000000 SUN)
- [x] Confirm the Java→Rust request mapping contract:
  - [x] `RemoteExecutionSPI` sends `tx.data = WitnessCreateContract.url` (URL bytes only)
  - [x] `tx.from` is the TRON-prefixed (21-byte) `owner_address`
  - [x] `tx.contract_parameter` carries the original `Any` (type_url + value) for `any.is(...)` parity

## 1) Implement `ALLOW_MULTI_SIGN` default witness permissions (core fix)

Goal: mirror Java `WitnessCreateActuator.createWitness()`:

```java
accountCapsule.setIsWitness(true);
if (dynamicStore.getAllowMultiSign() == 1) {
  accountCapsule.setDefaultWitnessPermission(dynamicStore);
}
```

Checklist (Rust):

- [x] In `BackendService::execute_witness_create_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [x] If `allow_multi_sign` is `true`, replicate `AccountCapsule.setDefaultWitnessPermission(...)`:
    - [x] Always set `account_proto.witness_permission = Some(Permission{...})`
    - [x] If `account_proto.owner_permission` is `None`, set default owner permission
    - [x] If `account_proto.active_permission` is empty, add default active permission
- [x] Match Java's defaults exactly (from `chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java`):
  - [x] Owner permission:
    - [x] `type = Owner`, `id = 0`, `permission_name = "owner"`, `threshold = 1`, `parent_id = 0`
    - [x] `keys = [{ address = ownerAddress(21 bytes), weight = 1 }]`
  - [x] Witness permission:
    - [x] `type = Witness`, `id = 1`, `permission_name = "witness"`, `threshold = 1`, `parent_id = 0`
    - [x] `keys = [{ address = ownerAddress(21 bytes), weight = 1 }]`
    - [x] `operations` should be empty / unset (Java does not populate it for witness permission)
  - [x] Active permission:
    - [x] `type = Active`, `id = 2`, `permission_name = "active"`, `threshold = 1`, `parent_id = 0`
    - [x] `operations = ACTIVE_DEFAULT_OPERATIONS` (32 bytes from dynamic props; Rust has `get_active_default_operations()`)
    - [x] `keys = [{ address = ownerAddress(21 bytes), weight = 1 }]`
- [x] Ensure the key address bytes match Java:
  - [x] Use the 21-byte TRON address (prefix + 20 bytes), not the raw 20-byte EVM address

## 2) Tests (prevent regression)

Goal: add Rust tests that would fail today and pass after the fix.

- [x] Add focused unit tests (Rust) for witness-create permissions:
  - [x] `test_witness_create_sets_default_permissions_when_multi_sign_enabled`:
    - [x] Setup: storage with an owner account that has no permission fields set.
    - [x] Execute witness-create with `ALLOW_MULTI_SIGN = 1`
    - [x] Assert `account_proto.is_witness == true`
    - [x] Assert `owner_permission` exists (id=0, type=Owner, threshold=1, key=owner weight=1)
    - [x] Assert `witness_permission` exists (id=1, type=Witness, threshold=1, key=owner weight=1)
    - [x] Assert `active_permission.len() >= 1` and the first entry matches the default (id=2, operations=ACTIVE_DEFAULT_OPERATIONS 32 bytes)
  - [x] `test_witness_create_no_permissions_when_multi_sign_disabled`:
    - [x] Execute witness-create with `ALLOW_MULTI_SIGN = 0`
    - [x] Assert `account_proto.is_witness == true`
    - [x] Assert the permission fields are unchanged (no new defaults injected)
  - [x] `test_witness_create_preserves_existing_owner_permission`:
    - [x] Pre-set owner_permission with custom threshold=2
    - [x] Execute witness-create with `ALLOW_MULTI_SIGN = 1`
    - [x] Assert owner_permission threshold is still 2 (not overwritten)
    - [x] Assert witness_permission is set (new)
  - [x] `test_witness_create_preserves_existing_active_permission`:
    - [x] Pre-set active_permission with custom name/threshold
    - [x] Execute witness-create with `ALLOW_MULTI_SIGN = 1`
    - [x] Assert active_permission is preserved (not replaced)
    - [x] Assert witness_permission is set (new)
- [ ] Java-side sanity check (not in scope for Rust-side changes):
  - [ ] Run `./gradlew :framework:test --tests "org.tron.core.actuator.WitnessCreateActuatorTest"`

## 3) Conformance / integration verification

- [x] Conformance fixtures for witness creation all pass:
  - [x] `WITNESS_CREATE_CONTRACT/edge_balance_equals_upgrade_cost` — PASS
  - [x] `WITNESS_CREATE_CONTRACT/edge_blackhole_optimization_burns_trx` — PASS
  - [x] `WITNESS_CREATE_CONTRACT/happy_path_create_witness` — PASS
  - [x] `WITNESS_CREATE_CONTRACT/validate_fail_insufficient_balance` — PASS
  - [x] `WITNESS_CREATE_CONTRACT/validate_fail_invalid_url` — PASS
  - [x] `WITNESS_CREATE_CONTRACT/validate_fail_owner_account_not_exist` — PASS
  - [x] `WITNESS_CREATE_CONTRACT/validate_fail_owner_address_invalid_empty` — PASS
  - [x] `WITNESS_CREATE_CONTRACT/validate_fail_url_too_long_257` — PASS
  - [x] `WITNESS_CREATE_CONTRACT/validate_fail_witness_exists` — PASS
- [x] Run Rust tests:
  - [x] `cd rust-backend && cargo test --workspace` — 426 passed, 3 failed (pre-existing vote_witness), 3 ignored
  - [x] All conformance fixtures pass (`./scripts/ci/run_fixture_conformance.sh --rust-only`)

## 4) Optional: investigate account-proto encoding parity on balance updates

This is broader than witness-create, but witness-create updates balances and can surface it.

- [ ] Evaluate whether `EngineBackedEvmStateStore::set_account()` must use the same java-compat encoding rewrite path as `put_account_proto()` (notably for `Account.asset_v2` map ordering/default-field encoding).
  - Note: Conformance tests pass without this fix, so this is low-priority and should be addressed repo-wide if needed.

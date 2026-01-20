# TODO / Fix Plan: `WITNESS_CREATE_CONTRACT` parity gaps

This checklist assumes we want Rust remote execution to match java-tron’s `WitnessCreateActuator` behavior, including the multi-sign permission side effects.

## 0) Confirm the parity target (do this first)

- [ ] Confirm the target environment’s dynamic properties (mainnet-like vs fixtures):
  - [ ] `ALLOW_MULTI_SIGN` (expected `1`?)
  - [ ] `ACTIVE_DEFAULT_OPERATIONS` value (32 bytes)
  - [ ] `ALLOW_BLACKHOLE_OPTIMIZATION` behavior
  - [ ] `ACCOUNT_UPGRADE_COST` value
- [ ] Confirm the Java→Rust request mapping contract:
  - [ ] `RemoteExecutionSPI` sends `tx.data = WitnessCreateContract.url` (URL bytes only)
  - [ ] `tx.from` is the TRON-prefixed (21-byte) `owner_address`
  - [ ] `tx.contract_parameter` carries the original `Any` (type_url + value) for `any.is(...)` parity

## 1) Implement `ALLOW_MULTI_SIGN` default witness permissions (core fix)

Goal: mirror Java `WitnessCreateActuator.createWitness()`:

```java
accountCapsule.setIsWitness(true);
if (dynamicStore.getAllowMultiSign() == 1) {
  accountCapsule.setDefaultWitnessPermission(dynamicStore);
}
```

Checklist (Rust):

- [ ] In `BackendService::execute_witness_create_contract()` (`rust-backend/crates/core/src/service/mod.rs`):
  - [ ] If `allow_multi_sign` is `true`, replicate `AccountCapsule.setDefaultWitnessPermission(...)`:
    - [ ] Always set `account_proto.witness_permission = Some(Permission{...})`
    - [ ] If `account_proto.owner_permission` is `None`, set default owner permission
    - [ ] If `account_proto.active_permission` is empty, add default active permission
- [ ] Match Java’s defaults exactly (from `chainbase/src/main/java/org/tron/core/capsule/AccountCapsule.java`):
  - [ ] Owner permission:
    - [ ] `type = Owner`, `id = 0`, `permission_name = "owner"`, `threshold = 1`, `parent_id = 0`
    - [ ] `keys = [{ address = ownerAddress(21 bytes), weight = 1 }]`
  - [ ] Witness permission:
    - [ ] `type = Witness`, `id = 1`, `permission_name = "witness"`, `threshold = 1`, `parent_id = 0`
    - [ ] `keys = [{ address = ownerAddress(21 bytes), weight = 1 }]`
    - [ ] `operations` should be empty / unset (Java does not populate it for witness permission)
  - [ ] Active permission:
    - [ ] `type = Active`, `id = 2`, `permission_name = "active"`, `threshold = 1`, `parent_id = 0`
    - [ ] `operations = ACTIVE_DEFAULT_OPERATIONS` (32 bytes from dynamic props; Rust has `get_active_default_operations()`)
    - [ ] `keys = [{ address = ownerAddress(21 bytes), weight = 1 }]`
- [ ] Ensure the key address bytes match Java:
  - [ ] Use the 21-byte TRON address (prefix + 20 bytes), not the raw 20-byte EVM address

## 2) Tests (prevent regression)

Goal: add Rust tests that would fail today and pass after the fix.

- [ ] Add a focused unit test (Rust) for witness-create permissions:
  - [ ] Setup: storage with an owner account that has no permission fields set.
  - [ ] Case A (`ALLOW_MULTI_SIGN = 1`):
    - [ ] Execute witness-create
    - [ ] Assert `account_proto.is_witness == true`
    - [ ] Assert `owner_permission` exists (id=0, type=Owner, threshold=1, key=owner weight=1)
    - [ ] Assert `witness_permission` exists (id=1, type=Witness, threshold=1, key=owner weight=1)
    - [ ] Assert `active_permission.len() >= 1` and the first entry matches the default (id=2, operations=ACTIVE_DEFAULT_OPERATIONS)
  - [ ] Case B (`ALLOW_MULTI_SIGN = 0`):
    - [ ] Execute witness-create
    - [ ] Assert `account_proto.is_witness == true`
    - [ ] Assert the permission fields are unchanged (no new defaults injected)
- [ ] Java-side sanity check:
  - [ ] Run `./gradlew :framework:test --tests "org.tron.core.actuator.WitnessCreateActuatorTest"`

## 3) Conformance / integration verification

- [ ] If conformance fixtures exist for witness voting, generate/validate witness-create cases:
  - [ ] `framework/src/test/java/org/tron/core/conformance/WitnessVotingFixtureGeneratorTest.java`
  - [ ] Ensure remote execution output (DB bytes / digests) matches embedded where expected.
- [ ] Run Rust tests:
  - [ ] `cd rust-backend && cargo test`

## 4) Optional: investigate account-proto encoding parity on balance updates

This is broader than witness-create, but witness-create updates balances and can surface it.

- [ ] Evaluate whether `EngineBackedEvmStateStore::set_account()` (in `rust-backend/crates/execution/src/storage_adapter/engine.rs`) must use the same java-compat encoding rewrite path as `put_account_proto()` (notably for `Account.asset_v2` map ordering/default-field encoding).
- [ ] If needed, plan a repo-wide change:
  - [ ] update `serialize_account_update` / `set_account` to preserve java-tron’s raw bytes for map fields
  - [ ] add fixture-based tests that assert raw Account bytes for accounts with ≥2 `asset_v2` entries


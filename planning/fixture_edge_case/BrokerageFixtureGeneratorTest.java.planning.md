Review Target

- `framework/src/test/java/org/tron/core/conformance/BrokerageFixtureGeneratorTest.java`

Scope

- Fixture generation for:
  - `UpdateBrokerageContract` (type 49)

Current Coverage (as written)

- Happy:
  - witness sets brokerage to `20%`
  - witness sets brokerage to `0%` (boundary)
  - witness sets brokerage to `100%` (boundary)
- Validate-fail:
  - owner is not a witness (no `WitnessStore` entry for owner)
  - brokerage < 0
  - brokerage > 100
  - change delegation feature disabled (`allowChangeDelegation == false`)
  - “account not exist” (but see note below)

Validation Branches (source of truth)

Validation path is in `actuator/src/main/java/org/tron/core/actuator/UpdateBrokerageActuator.java`.

- Feature flag gate:
  - `!dynamicStore.allowChangeDelegation()` → throws
    `"contract type error, unexpected type [UpdateBrokerageContract]"`
- Contract payload/type gate:
  - `!any.is(UpdateBrokerageContract.class)` → throws
    `"contract type error, expected type [UpdateBrokerageContract], real type[...]"`
  - unpack failure → throws `InvalidProtocolBufferException` message
- Owner address validation:
  - `!DecodeUtil.addressValid(ownerAddress)` → throws `"Invalid ownerAddress"`
- Brokerage validation:
  - `brokerage < 0 || brokerage > 100` → throws `"Invalid brokerage"`
- Existence checks (order matters):
  - `witnessStore.get(ownerAddress) == null` → throws `"Not existed witness:<hex>"`
  - `accountStore.get(ownerAddress) == null` → throws `"Account does not exist"`

Missing Edge Cases (high value for conformance)

- Invalid `ownerAddress` (fails `DecodeUtil.addressValid`):
  - empty bytes (`ByteString.EMPTY`)
  - wrong length (not 21 bytes)
  - wrong prefix byte (21 bytes but not current network prefix)
  - Expected validation error: `"Invalid ownerAddress"`

- Account missing *after* witness exists (covers `"Account does not exist"` branch):
  - Requires seeding `WitnessStore` for the owner while *not* seeding `AccountStore` for the same owner.
  - Expected validation error: `"Account does not exist"`

- Optional encoding/type edge fixtures (lower priority):
  - Transaction contract type is `UpdateBrokerageContract` but the `parameter` packs a different proto message
    (covers `!any.is(UpdateBrokerageContract.class)` branch).
  - `Any` has `type_url` for `UpdateBrokerageContract` but invalid `value` bytes
    (covers unpack/`InvalidProtocolBufferException` branch).

Notes / Potential “false coverage” in current tests

- `generateUpdateBrokerage_accountNotExist` likely does **not** reach `"Account does not exist"`:
  - `UpdateBrokerageActuator.validate()` checks `WitnessStore` first, so a fresh random address fails with
    `"Not existed witness:..."` before it ever checks `AccountStore`.
  - The fixture currently labels this as “account does not exist”, but it is effectively another “witness missing”
    case unless the test explicitly inserts a witness for that address.

- `generateUpdateBrokerage_changeDelegationDisabled` throws a “contract type error … unexpected type” message:
  - The message does not mention delegation; consumers that match on substrings should rely on the actual
    `expectedErrorMessage` captured in `metadata.json`.

- Fixture-generation hygiene differences vs other conformance generator tests:
  - `createTransaction()` uses `System.currentTimeMillis()` and does not populate `feeLimit/refBlock*`;
    other generator tests typically use `ConformanceFixtureTestSupport.createTransaction(...)` for deterministic,
    fully-populated raw data.
  - `createBlockContext()` does not set parent hash or persist the new head hash/number/time into
    `DynamicPropertiesStore` (unlike `ConformanceFixtureTestSupport.createBlockContext(dbManager, ...)`).

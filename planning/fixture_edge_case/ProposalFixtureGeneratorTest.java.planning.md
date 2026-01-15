Review Target

- `framework/src/test/java/org/tron/core/conformance/ProposalFixtureGeneratorTest.java`

Scope

- Fixture generation for:
  - `ProposalCreateContract` (type 16)
  - `ProposalApproveContract` (type 17)
  - `ProposalDeleteContract` (type 18)

Current Coverage (as written)

ProposalCreateContract (16)

- Happy: create a proposal with a valid parameter (`MAINTENANCE_TIME_INTERVAL`).
- Validate-fail: owner account exists but is not a witness.
- Validate-fail: empty parameters map.
- Happy: create a proposal with multiple valid parameters (0/1/2).

ProposalApproveContract (17)

- Happy: approve an existing proposal.
- Validate-fail: approve a non-existent proposal (`proposalId > latestProposalNum`).
- Happy: remove approval (`isAddApproval=false`) after prior approval.
- Validate-fail: repeat approval by the same witness.
- Validate-fail: remove approval when the witness never approved.
- Validate-fail: approve an expired proposal.
- Validate-fail: approve a canceled proposal.

ProposalDeleteContract (18)

- Happy: delete an existing proposal by the creator.
- Validate-fail: delete by a different account (not the proposer).
- Validate-fail: delete a canceled proposal.
- Validate-fail: delete a non-existent proposal (`proposalId > latestProposalNum`).
- Validate-fail: delete an expired proposal.

Missing Edge Cases (high value for conformance)

ProposalCreateContract

- Validate path: `actuator/src/main/java/org/tron/core/actuator/ProposalCreateActuator.java`
- Parameter validation: `actuator/src/main/java/org/tron/core/utils/ProposalUtil.java`

Missing owner-address validation branches

- Invalid `ownerAddress` bytes (fails `DecodeUtil.addressValid`):
  - empty / wrong length (not 21 bytes) / wrong prefix byte.
- Owner account missing (fails account existence check) distinct from “not witness”.

Missing parameter id / value validation branches (fork-independent)

- Unsupported parameter code (`ProposalType.getEnum` throws `Does not support code : X`).
- `MAINTENANCE_TIME_INTERVAL` out of range:
  - below min (`< 3 * 27 * 1000`) and above max (`> 24 * 3600 * 1000`).
- Negative value for a non-negative parameter (`ACCOUNT_UPGRADE_COST`, `CREATE_ACCOUNT_FEE`, etc.).
- Value exceeds `LONG_VALUE` for long-range parameters.
- “Only allowed to be 1” parameters:
  - e.g., `ALLOW_CREATION_OF_CONTRACTS` (9) with value `0`.

Missing parameter dependency / prerequisite branch (fork-independent)

- `ALLOW_TVM_TRANSFER_TRC10` (18) requires `DynamicPropertiesStore.getAllowSameTokenName() == 1`;
  add a fixture where it is `0` to cover:
  - `"[ALLOW_SAME_TOKEN_NAME] proposal must be approved before [ALLOW_TVM_TRANSFER_TRC10] can be proposed"`.

Missing one-time-only proposal validation branches (fork-independent)

- `REMOVE_THE_POWER_OF_THE_GR` (10):
  - when `getRemoveThePowerOfTheGr() == -1` (“only allowed once”),
  - when value != 1 (“only allowed to be 1”).

Optional boundary-happy fixtures

- `MAINTENANCE_TIME_INTERVAL` exactly at min and max bounds (locks inclusive boundary behavior).

ProposalApproveContract

- Validate path: `actuator/src/main/java/org/tron/core/actuator/ProposalApproveActuator.java`

Missing owner/witness validation branches

- Invalid `ownerAddress` bytes (“Invalid address”).
- Owner account missing (“Account[...] not exists”).
- Owner witness missing (“Witness[...] not exists”).

Missing proposal-store/dynamic-property consistency branch

- `latestProposalNum >= proposalId` but proposal missing in `ProposalStore` (hits `ItemNotFoundException`
  branch rather than the `proposalId > latestProposalNum` guard).

Missing expiration boundary fixture

- `now == expirationTime` must fail (code uses `now >= expirationTime`).

ProposalDeleteContract

- Validate path: `actuator/src/main/java/org/tron/core/actuator/ProposalDeleteActuator.java`

Missing owner-address/account existence branches

- Invalid `ownerAddress` bytes (“Invalid address”).
- Owner account missing (“Account[...] not exists”).

Missing proposal-store/dynamic-property consistency branch

- `latestProposalNum >= proposalId` but proposal missing in `ProposalStore`.

Missing expiration boundary fixture

- `now == expirationTime` must fail (code uses `now >= expirationTime`).

Important semantic nuance currently not pinned by fixtures

- `ProposalDeleteActuator.validate()` does not require witness membership (only account existence + proposer match).
  Add a “witness entry removed but delete still succeeds” fixture to catch accidental “witness required”
  implementations elsewhere.

Notes / Potential Test-Harness Gaps

- `FixtureGenerator` overwrites `expectedStatus/expectedErrorMessage` based on real actuator results.
  Without assertions on `FixtureResult` (success vs validation error), tests can silently drift from their
  intended branch and still “pass” while generating the wrong fixture.
- Many proposal parameters are fork-gated via `ForkController.pass(...)`; avoid relying on these in fixtures
  unless the fork state is explicitly configured (otherwise fixtures become environment-dependent).


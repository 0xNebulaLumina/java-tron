• I’m going to map both commits to the two planning docs and call out any gaps.

  Summary

  - Commit 32047c9 implements Phase 1 scaffolding: Java TRC‑10 mapping, backend.proto additions, Rust→proto conversion hooks, and config flag.
  - Commit b06a76e completes Phase 1 direction: Java apply path added in RuntimeSpiImpl, Rust handlers for ISSUE/PARTICIPATE with manual protobuf parsing, and TODO doc updated.
  - The work aligns with asset_issue.todo.md (Phase 1 “emit-and-apply”). It does not follow the “persist in Rust” choice stated in asset_issue.planning.md (which preferred direct Rust persistence).
  - There are important Java plumbing gaps and parity issues to address before this is complete.

  What matches the TODO plan

  - Java mapping in RemoteExecutionSPI
      - AssetIssueContract and ParticipateAssetIssueContract are mapped, gated by -Dremote.exec.trc10.enabled with fallback to Java actuators:
          - AssetIssue: from=owner, to empty, value=0, data=full bytes, txKind=NON_VM, contract_type=ASSET_ISSUE_CONTRACT, asset_id empty. framework/src/main/java/org/tron/core/execution/spi/
            RemoteExecutionSPI.java:413
          - Participate: from=owner, to=to_address, value=0, data=full bytes, txKind=NON_VM, contract_type=PARTICIPATE_ASSET_ISSUE_CONTRACT, asset_id=asset_name. framework/src/main/java/org/tron/core/execution/
            spi/RemoteExecutionSPI.java:434
      - TRC‑10 transfer remains gated and stubbed on Rust side. framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:303
      - Pre‑exec AEXT snapshots include Participate recipient. framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:948
  - Proto additions
      - Trc10Op, FrozenSupply, Trc10LedgerChange, and repeated trc10_changes are defined. framework/src/main/proto/backend.proto:704, 711, 717, 615
  - Rust backend
      - Dispatch arms add gating and route to handlers. rust-backend/crates/core/src/service/mod.rs:240
      - Handlers for ASSET_ISSUE and PARTICIPATE parse payloads manually, validate basics, and emit Trc10LedgerChange. rust-backend/crates/core/src/service/mod.rs:1135, 1510
      - Conversion maps internal trc10_changes to protobuf. rust-backend/crates/core/src/service/grpc/conversion.rs:390
      - Config flag execution.remote.trc10_enabled=true for dev/testing, with log at startup. rust-backend/config.toml:116, rust-backend/src/main.rs:39
  - Java apply path and toggle
      - applyTrc10LedgerChanges added and wired in RuntimeSpiImpl.execute(). framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:82, 435
      - Rollback toggle -Dremote.exec.apply.trc10 (default=true). framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:439

  Gaps and misalignments

  - Missing Java plumbing: trc10_changes aren’t surfaced from gRPC into the Java result
      - RemoteExecutionSPI.convertExecuteTransactionResponse doesn’t read trc10_changes from the protobuf result, and ExecutionSPI.ExecutionResult lacks a trc10 field. framework/src/main/java/org/tron/core/
        execution/spi/RemoteExecutionSPI.java: convertExecuteTransactionResponse; framework/src/main/java/org/tron/core/execution/spi/ExecutionSPI.java:1
      - ExecutionProgramResult has no trc10Changes field or accessor. framework/src/main/java/org/tron/core/execution/spi/ExecutionProgramResult.java:1
      - RuntimeSpiImpl calls result.getTrc10Changes() and references ExecutionSPI.Trc10LedgerChange and ExecutionSPI.FrozenSupply, but those types/methods don’t exist yet, so this won’t compile/run as-is.
        framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:449, 464, 542
  - Java apply semantics don’t yet mirror AssetIssue/Participate actuators
      - Asset issuance:
          - Uses totalSupply to credit owner; should credit remainSupply = totalSupply − sum(frozen). framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:586
          - Doesn’t deduct ASSET_ISSUE_FEE or handle burn vs blackhole. Actuator does. actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java:97
          - Doesn’t honor ALLOW_SAME_TOKEN_NAME for store writes (V1 name-keyed store vs V2 only) or set precision=0 in V1 mode. actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java:85
          - Doesn’t set account’s issued asset references (setAssetIssuedName, setAssetIssuedID) nor add account frozenSupply list. actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java:121
      - Participate:
          - Calculates token amount but skips full validation parity (time window, issuer match, balance checks) and ledger updates for asset V1/V2 differences; current logic updates V2 map but may not handle
            name-keyed assets under ALLOW_SAME_TOKEN_NAME=0. framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java:608
  - Divergence from planning/asset_issue.planning.md
      - The planning file selected “persist in Rust” to minimize Java-side changes; these commits implemented the TODO’s Phase 1 “emit-and-apply” instead. This is acceptable if the plan was revised, but it’s
        not aligned with the original “direct persistence in Rust” choice in that document.

  What to fix next

  - Plumb TRC‑10 changes through Java
      - Add List<Trc10LedgerChange> trc10Changes to ExecutionSPI.ExecutionResult and ExecutionProgramResult with getters/setters. framework/src/main/java/org/tron/core/execution/spi/ExecutionSPI.java,
        framework/src/main/java/org/tron/core/execution/spi/ExecutionProgramResult.java
      - In RemoteExecutionSPI.convertExecuteTransactionResponse, read protoResult.getTrc10ChangesList() and populate the Java list. framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java:
        convertExecuteTransactionResponse
      - Define Java-side ExecutionSPI.Trc10LedgerChange, ExecutionSPI.Trc10Op, and ExecutionSPI.FrozenSupply DTOs (or reuse the generated protobuf types and translate once).
  - Align apply logic with actuators
      - AssetIssue: compute remainSupply, ALLOW_SAME_TOKEN_NAME handling, precision adjustment in V1, update issued asset refs on account, add account frozenSupply, deduct ASSET_ISSUE_FEE (burn or credit
        blackhole). actuator/src/main/java/org/tron/core/actuator/AssetIssueActuator.java:67
      - Participate: look up correct asset via name or id depending on ALLOW_SAME_TOKEN_NAME, enforce time window and issuer rules, compute exchangeAmount floor exactly like actuator, debit/credit TRX and asset
        balances correctly (V1/V2).
  - Tests (as per TODO)
      - Unit tests for mapping fallback and Java apply logic with synthetic ExecutionProgramResult.trc10Changes.
      - Rust handler unit tests for ISSUE/PARTICIPATE happy paths and gating.

  Bottom line

  - Compliance with asset_issue.todo.md: Partially complete. The mapping, proto, Rust handlers, and Java apply entry point are in place, but Java result plumbing for trc10_changes and actuator-parity
    application logic are incomplete.
  - Compliance with asset_issue.planning.md: Not following the “persist in Rust” path chosen there; instead, it follows the TODO’s Phase 1 “emit-and-apply” strategy.

---


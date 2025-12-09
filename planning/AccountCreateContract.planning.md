• Here’s a concrete, repo-aware plan to add AccountCreateContract handling to rust-backend with parity to Java’s CreateAccountActuator.

Current State

- Dispatch: Non-VM handler exists with explicit matches for many contract types, but no AccountCreateContract case yet (rust-backend/crates/core/src/service/mod.rs:195).
- Fee + blackhole: Witness/asset handlers already implement owner-fee deduction and optional blackhole credit with dynamic props (rust-backend/crates/core/src/service/mod.rs:651).
- Dynamic props: Engine-backed store reads many props, but not CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT yet (rust-backend/crates/execution/src/storage_adapter/engine.rs:260).
- Proto parsing: Helpers exist for varints and selective proto parsing (rust-backend/crates/core/src/service/contracts/proto.rs:1).
- Java reference semantics: CreateAccountActuator validates owner/account addresses, owner existence, account non-existence, and fee sufficiency; deducts fee and burns/credits blackhole (actuator/src/main/
java/org/tron/core/actuator/CreateAccountActuator.java:23).

Semantics To Mirror

- Validation:
    - any.is(AccountCreateContract) and ownerAddress/accountAddress validity/format.
    - Owner account must exist; target account must not exist.
    - Owner balance >= getCreateNewAccountFeeInSystemContract() or fail with “Validate CreateAccountActuator error, insufficient fee.”
- Execution:
    - Create new account with defaults (Normal type, zero balance, create_time).
    - Deduct fee from owner; burn (blackhole optimization) or credit blackhole.
- Reporting:
    - energy_used = 0, bandwidth_used based on tx payload.
    - State changes: owner AccountChange (balance delta), new AccountChange with is_creation; optional blackhole AccountChange when crediting.
    - AEXT: hybrid/tracked mode should include owner; new account can use defaults.

Backend Changes

- Config flag
    - Add execution.remote.account_create_enabled: bool (default false) in config:
        - rust-backend/crates/common/src/config.rs: add field + default, load defaults, and wiring.
        - rust-backend/config.toml: add account_create_enabled = true/false.
        - rust-backend/src/main.rs: log the flag at startup (rust-backend/src/main.rs:51).
- Dynamic property getters
    - Add getter for CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT:
        - rust-backend/crates/execution/src/storage_adapter/engine.rs: implement get_create_new_account_fee_in_system_contract() reading “CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT” as big-endian u64 (pattern
        like get_asset_issue_fee()).
- Proto parsing
    - Implement a lightweight parser for AccountCreateContract in the service:
        - Fields: owner_address (ignored, use tx.from), account_address (bytes), type (varint).
        - Place in rust-backend/crates/core/src/service/mod.rs near other parse_* helpers, or add a small helper in contracts/proto.rs.
- Dispatch
    - Add match arm for AccountCreateContract that checks the new flag and calls the handler:
        - rust-backend/crates/core/src/service/mod.rs:195.

Handler Logic

- Implement execute_account_create_contract in rust-backend/crates/core/src/service/mod.rs:
    - Inputs: storage_adapter, transaction, context.
    - Steps:
        1. Extract target account_address:
            - Preferred: parse from transaction.data as AccountCreateContract; fallback to transaction.to if we decide to carry the new address there.
        2. Validate:
            - Owner exists: get_account(&transaction.from) must be Some.
            - Parse/validate target address bytes (expect 21-byte Tron address with 0x41 prefix or convertable; use grpc/address::strip_tron_address_prefix if you have 21 bytes).
            - Target account must not exist: get_account(&new_addr) is None.
        3. Fee:
            - Read fee via get_create_new_account_fee_in_system_contract(). If owner.balance < fee, error “Validate CreateAccountActuator error, insufficient fee.”
        4. Apply:
            - new_owner = owner with balance - fee; persist via set_account(from, new_owner).
            - new_account = AccountInfo { balance: 0, nonce: 0, code_hash: ZERO, code: None }; persist via set_account(new_addr, new_account).
            - Blackhole handling:
                - If support_black_hole_optimization() is true, burn (no blackhole credit).
                - Else, credit blackhole:
                    - Load blackhole account (dynamic property or default mainnet), add fee, persist, emit AccountChange.
        5. State changes:
            - AccountChange for owner (old -> new).
            - AccountChange for new account (old=None, new=Some).
            - Optional AccountChange for blackhole (credit path).
            - Sort deterministically by address (pattern used elsewhere).
        6. Bandwidth and AEXT:
            - bandwidth_used = calculate_bandwidth_usage(transaction).
            - If accountinfo_aext_mode == “tracked”, use ResourceTracker for the owner and persist updated AEXT; include aext_map entry for owner.
        7. Return TronExecutionResult with success, zero energy, bandwidth, state_changes, aext_map, empty logs and sidecars.

Notes:

- Create time parity: current account serializer uses system time (engine.rs: serialize_account). For better determinism, consider enhancing set_account/serializer to use context.block_timestamp instead of
SystemTime::now() for create_time. If changing serializer is too invasive now, document the divergence and plan a follow-up patch.
- Account type: Java passes type in AccountCreateContract, but serializer currently hardcodes Normal. It’s acceptable initially; annotate a TODO to optionally set type when serializer supports it.

Parsing Inputs

- Minimal parser for AccountCreateContract:
    - Read field 2 (account_address, length-delimited), field 3 (type, varint).
    - Use contracts::proto::read_varint (rust-backend/crates/core/src/service/contracts/proto.rs:1).
    - Convert 21-byte Tron to 20-byte EVM with grpc/address::strip_tron_address_prefix (rust-backend/crates/core/src/service/grpc/address.rs).
- Fallback if data empty:
    - Optionally treat transaction.to as the target account if RemoteExecutionSPI sets it that way.

Fee Handling

- Use the same pattern as WitnessCreate and AssetIssue:
    - Read owner, compute fee U256, deduct, persist owner.
    - Blackhole optimization:
        - If enabled, burn (no state delta).
        - Else, credit blackhole account (persist and add state change).
- Keys:
    - CREATE_NEW_ACCOUNT_FEE_IN_SYSTEM_CONTRACT (read as u64 SUN).
    - ALLOW_BLACKHOLE_OPTIMIZATION for burn vs credit (already supported).

AEXT/Bandwidth

- Bandwidth: reuse Self::calculate_bandwidth_usage(transaction) (rust-backend/crates/core/src/service/mod.rs:300).
- AEXT: in tracked mode, update/persist owner’s AEXT via ResourceTracker and include owner in aext_map, like other handlers (see patterns at rust-backend/crates/core/src/service/mod.rs:1220).

Java Glue (minimal awareness)

- RemoteExecutionSPI currently does not build a request for AccountCreateContract.
- Plan to extend:
    - In buildExecuteTransactionRequest, add a case for CreateAccount:
        - Set contractType = ACCOUNT_CREATE_CONTRACT.
        - Place full AccountCreateContract proto bytes into tx.data (consistent with AssetIssue parsing pattern).
    - Pre-exec AEXT: owner snapshot is already included; no need for the new account snapshot.
    - Location: framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java.

Tests

- Add service-layer unit tests in rust-backend/crates/core/src/service/tests/contracts.rs:
    - test_account_create_happy_path:
        - With owner (sufficient balance), fee>0, blackhole disabled (burn) → 2 state changes, zero energy, >0 bandwidth.
    - test_account_create_blackhole_credit:
        - support_black_hole_optimization=false and blackhole configured → 3 state changes; verify blackhole balance delta.
    - test_account_create_insufficient_fee:
        - Expect exact error string “Validate CreateAccountActuator error, insufficient fee.”
    - test_account_create_existing_account:
        - Target exists → “Account has existed”.
    - test_account_create_missing_owner:
        - “account <owner> not exist” style message (mirror Java wording where practical).
- Optional: validate deterministic ordering of state changes and CSV parity characteristics (no logs, etc.).

Edge Cases

- Address bytes:
    - Accept 21-byte Tron or error clearly; refuse malformed sizes.
- Fee=0 networks:
    - Permit creation with no fee (state change owner old==new to carry AEXT if necessary).
- Account type:
    - Ignore non-Normal initial type for now; document as follow-up enhancement.
- Create time:
    - If serializer continues using system time, call out time-drift risk in test expectations.

Rollout

- Behind execution.remote.account_create_enabled (default false). If disabled, service returns a friendly error prompting Java to fall back to embedded actuator.
- Log configuration on startup for visibility (rust-backend/src/main.rs:51).
- Keep blackhole behavior parity via dynamic property and fee mode; ensure logs reflect credit/burn decisions.



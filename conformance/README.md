# Conformance Testing Framework

This directory contains the fixture-based conformance testing framework for verifying Rust backend execution parity with Java's embedded actuator execution.

## Overview

The conformance framework enables "red-light/green-light" testing for contract implementations:
1. **Java generates golden fixtures** - Using embedded actuators as the oracle
2. **Rust runs conformance tests** - Comparing execution results against fixtures
3. **CI enforces parity** - PR gate and nightly regression tests

## Directory Structure

```
conformance/
├── README.md                    # This file
├── fixtures/                    # Generated test fixtures
│   └── <contract_type>/         # One directory per contract type
│       └── <case_name>/         # One directory per test case
│           ├── metadata.json    # Test case metadata
│           ├── request.pb       # ExecuteTransactionRequest protobuf bytes
│           ├── pre_db/          # Pre-execution database state
│           │   └── <db_name>.kv # Key-value pairs (binary format)
│           └── expected/        # Expected post-execution state
│               ├── post_db/     # Post-execution database state
│               │   └── <db_name>.kv
│               └── result.pb    # ExecutionResult protobuf bytes
├── schema/                      # Schema definitions
│   ├── kv_format.md            # KV file format specification
│   └── metadata_schema.json    # JSON schema for metadata.json
└── generator/                   # Java fixture generator (in framework/src/test)
```

## Fixture Schema

### metadata.json

```json
{
  "contract_type": "PROPOSAL_CREATE_CONTRACT",
  "contract_type_num": 16,
  "case_name": "happy_path_create",
  "case_category": "happy",
  "description": "Create a new proposal successfully",
  "generated_at": "2025-01-15T10:30:00Z",
  "generator_version": "1.0.0",
  "block_number": 1000,
  "block_timestamp": 1705312200000,
  "databases_touched": ["account", "proposal", "dynamic-properties", "witness"]
}
```

### KV File Format (*.kv)

Binary format for deterministic key-value storage:
- Header: 4-byte magic "KVDB" + 4-byte version (big-endian uint32)
- Entry count: 4-byte big-endian uint32
- Entries (sorted by key lexicographically):
  - Key length: 4-byte big-endian uint32
  - Key bytes
  - Value length: 4-byte big-endian uint32
  - Value bytes (empty value = 0 length)

This format ensures:
- Cross-platform stability (no JSON encoding issues for binary data)
- Deterministic ordering (sorted keys)
- Efficient comparison (can compare files byte-by-byte)

## Test Case Categories

Each contract should have fixtures covering:

1. **happy** - Successful execution path
2. **validate_fail** - Validation failure (should fail without state changes)
3. **edge** - Edge cases (boundaries, limits, special values)

## Usage

### Generating Fixtures (Java)

```bash
# Generate all fixtures
./gradlew :framework:test --tests "FixtureGenerator*"

# Generate fixtures for specific contract
./gradlew :framework:test --tests "FixtureGenerator*" -Dconformance.contract=PROPOSAL_CREATE_CONTRACT
```

### Running Conformance Tests (Rust)

```bash
cd rust-backend
cargo test --features conformance conformance_tests
```

### CI Integration

- **PR Gate**: Runs fixture conformance for all implemented contracts
- **Nightly**: Full regression with live chain replay

## Comparison Dimensions

The Rust runner compares:

1. **Required**: Post-DB bytes must match exactly (per touched database)
2. **Required**: Receipt fields (from ExecutionResult.tron_transaction_result)
3. **Optional**: State change digest (SHA-256 of canonical state changes)

## Adding New Contract Types

1. Create Java test cases in `framework/src/test/.../conformance/`
2. Implement contract handler in Rust
3. Run fixture generator
4. Run Rust conformance tests
5. Iterate until all tests pass

## Contract Coverage Status

| Contract Type | Num | Java Generator | Rust Runner | Status |
|--------------|-----|----------------|-------------|--------|
| PROPOSAL_CREATE | 16 | [ ] | [ ] | Planned |
| PROPOSAL_APPROVE | 17 | [ ] | [ ] | Planned |
| PROPOSAL_DELETE | 18 | [ ] | [ ] | Planned |
| SET_ACCOUNT_ID | 19 | [ ] | [ ] | Planned |
| ACCOUNT_PERMISSION_UPDATE | 46 | [ ] | [ ] | Planned |
| UPDATE_SETTING | 33 | [ ] | [ ] | Planned |
| UPDATE_ENERGY_LIMIT | 45 | [ ] | [ ] | Planned |
| CLEAR_ABI | 48 | [ ] | [ ] | Planned |
| UPDATE_BROKERAGE | 49 | [ ] | [ ] | Planned |
| WITHDRAW_EXPIRE_UNFREEZE | 56 | [ ] | [ ] | Planned |
| CANCEL_ALL_UNFREEZE_V2 | 59 | [ ] | [ ] | Planned |
| DELEGATE_RESOURCE | 57 | [ ] | [ ] | Planned |
| UNDELEGATE_RESOURCE | 58 | [ ] | [ ] | Planned |

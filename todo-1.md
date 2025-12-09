# Option 1: Full Rust Implementation - Detailed Checklist

## Overview
Implement complete `withdrawReward` logic in Rust, porting `MortgageService.withdrawReward()` from Java.

---

## Phase 1: Data Structures and Types

### 1.1 Define Core Types
- [x] Create `rust-backend/crates/execution/src/delegation/mod.rs`
- [x] Define `Vote` struct (as `DelegationVote`)
  ```rust
  pub struct DelegationVote {
      pub vote_address: Address,  // 20-byte witness address
      pub vote_count: i64,
  }
  ```
- [x] Define `AccountVoteSnapshot` struct
  ```rust
  pub struct AccountVoteSnapshot {
      pub address: Address,
      pub votes: Vec<DelegationVote>,
  }
  ```
- [x] Define constants
  ```rust
  pub const DELEGATION_STORE_REMARK: i64 = -1;
  pub const DECIMAL_OF_VI_REWARD: u128 = 1_000_000_000_000_000_000; // 10^18
  pub const DEFAULT_BROKERAGE: i32 = 20;
  ```
- [x] Add `num-bigint` crate to `Cargo.toml` for BigInt support
- [x] Export module from `execution/src/lib.rs`

### 1.2 Implement Protobuf Parsing for Votes
- [x] Add method to parse votes from Account protobuf
  ```rust
  fn parse_account_votes(account_bytes: &[u8]) -> Result<Vec<Vote>, String>
  ```
- [x] Handle protobuf field 5 (repeated Vote votes) in Account message
- [x] Test vote parsing with sample account data

---

## Phase 2: Storage Key Generation

### 2.1 Identify Java Key Formats
- [x] Read `DelegationStore.java` to understand key formats
- [x] Document key format for `begin_cycle`: raw address bytes (21-byte Tron format)
- [x] Document key format for `end_cycle`: `"end-{hex(address)}"`
- [x] Document key format for `account_vote`: `"{cycle}-{hex(address)}-account-vote"`
- [x] Document key format for `reward`: `"{cycle}-{hex(address)}-reward"`
- [x] Document key format for `witness_vote`: `"{cycle}-{hex(address)}-vote"`
- [x] Document key format for `witness_vi`: `"{cycle}-{hex(address)}-vi"`
- [x] Document key format for `brokerage`: `"{cycle}-{hex(address)}-brokerage"`

### 2.2 Implement Key Generation in Rust
- [x] Create `rust-backend/crates/execution/src/delegation/keys.rs`
- [x] Implement `delegation_begin_cycle_key(address: &[u8]) -> Vec<u8>`
- [x] Implement `delegation_end_cycle_key(address: &[u8]) -> Vec<u8>`
- [x] Implement `delegation_account_vote_key(cycle: i64, address: &[u8]) -> Vec<u8>`
- [x] Implement `delegation_reward_key(cycle: i64, witness: &[u8]) -> Vec<u8>`
- [x] Implement `delegation_witness_vote_key(cycle: i64, witness: &[u8]) -> Vec<u8>`
- [x] Implement `delegation_witness_vi_key(cycle: i64, witness: &[u8]) -> Vec<u8>`
- [x] Implement `delegation_brokerage_key(cycle: i64, witness: &[u8]) -> Vec<u8>`
- [x] Add unit tests comparing generated keys with Java

---

## Phase 3: Storage Adapter - Read Methods

### 3.1 Dynamic Properties Access
- [x] Add `allow_change_delegation(&self) -> Result<bool, String>` to `EngineBackedEvmStateStore`
  - [x] Read key `ALLOW_CHANGE_DELEGATION` from dynamic properties
  - [x] Return false if not found (default)
- [x] Add `get_current_cycle_number(&self) -> Result<i64, String>`
  - [x] Read key `CURRENT_CYCLE_NUMBER` from dynamic properties
  - [x] Parse as i64 (big-endian)
- [x] Add `get_new_reward_algorithm_effective_cycle(&self) -> Result<i64, String>`
  - [x] Read key `NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE`
  - [x] Return i64::MAX if not found (old algorithm always)

### 3.2 Delegation Store Read Methods
- [x] Add `get_delegation_begin_cycle(&self, address: &Address) -> Result<i64, String>`
  - [x] Generate key using `delegation_begin_cycle_key`
  - [x] Read from delegation store database
  - [x] Parse as i64, default to 0 if not found
- [x] Add `get_delegation_end_cycle(&self, address: &Address) -> Result<i64, String>`
  - [x] Generate key using `delegation_end_cycle_key`
  - [x] Read from delegation store database
  - [x] Parse as i64, default to REMARK (-1) if not found
- [x] Add `get_delegation_account_vote(&self, cycle: i64, address: &Address) -> Result<Option<AccountVoteSnapshot>, String>`
  - [x] Generate key using `delegation_account_vote_key`
  - [x] Read from delegation store database
  - [x] Parse Account protobuf, extract votes
  - [x] Return None if not found
- [x] Add `get_delegation_reward(&self, cycle: i64, witness: &Address) -> Result<i64, String>`
  - [x] Generate key using `delegation_reward_key`
  - [x] Read from delegation store database
  - [x] Parse as i64, default to 0
- [x] Add `get_delegation_witness_vote(&self, cycle: i64, witness: &Address) -> Result<i64, String>`
  - [x] Generate key using `delegation_witness_vote_key`
  - [x] Read from delegation store database
  - [x] Parse as i64, handle REMARK value (-1)
- [x] Add `get_delegation_witness_vi(&self, cycle: i64, witness: &Address) -> Result<BigInt, String>`
  - [x] Generate key using `delegation_witness_vi_key`
  - [x] Read from delegation store database
  - [x] Parse as BigInt (Java's two's complement format)
- [x] Add `get_delegation_brokerage(&self, cycle: i64, witness: &Address) -> Result<i32, String>`
  - [x] Generate key using `delegation_brokerage_key`
  - [x] Read from delegation store database
  - [x] Parse as i32, default to 20 (20% default brokerage)

### 3.3 Database Routing
- [x] Identify delegation store database name in Rust storage service: "delegation"
- [x] Add delegation store to database routing in storage adapter
- [ ] Ensure gRPC storage service can access delegation store
- [ ] Test basic read operations against delegation store

---

## Phase 4: Storage Adapter - Write Methods

### 4.1 Delegation Store Write Methods
- [x] Add `set_delegation_begin_cycle(&self, address: &Address, cycle: i64) -> Result<(), String>`
  - [x] Generate key using `delegation_begin_cycle_key`
  - [x] Serialize cycle as i64 big-endian (match Java format)
  - [x] Write to delegation store database
- [x] Add `set_delegation_end_cycle(&self, address: &Address, cycle: i64) -> Result<(), String>`
  - [x] Generate key using `delegation_end_cycle_key`
  - [x] Serialize cycle as i64 big-endian
  - [x] Write to delegation store database
- [x] Add `set_delegation_account_vote(&self, cycle: i64, address: &Address, snapshot: &AccountVoteSnapshot) -> Result<(), String>`
  - [x] Generate key using `delegation_account_vote_key`
  - [x] Serialize account snapshot to protobuf
  - [x] Write to delegation store database

### 4.2 Track State Changes
- [ ] Add delegation store changes to `TronStateChange` enum (deferred - delegation writes go directly to storage)
  ```rust
  enum TronStateChange {
      // ... existing variants ...
      DelegationChange {
          key: Vec<u8>,
          old_value: Option<Vec<u8>>,
          new_value: Option<Vec<u8>>,
      }
  }
  ```
- [ ] Emit delegation changes for CSV parity (or gate behind config) - deferred to Phase 2

---

## Phase 5: Reward Computation - Core Logic

### 5.1 Main withdrawReward Function
- [x] Create `rust-backend/crates/core/src/service/contracts/delegation.rs`
- [x] Implement `withdraw_reward(storage: &EngineBackedEvmStateStore, address: &Address) -> Result<i64, String>`
  - [x] Check `allow_change_delegation()`, return 0 if false
  - [x] Get account, return 0 if not found
  - [x] Get `begin_cycle`, `end_cycle`, `current_cycle`
  - [x] Return 0 if `begin_cycle > current_cycle`
  - [x] Handle same-cycle check (begin == current)
  - [x] Handle latest cycle reward withdrawal
  - [x] Compute remaining cycle rewards
  - [x] Update delegation store state
  - [x] Return total computed reward

### 5.2 computeReward Function
- [x] Implement `compute_reward(storage: &EngineBackedEvmStateStore, begin: i64, end: i64, account: &AccountVoteSnapshot) -> Result<i64, String>`
  - [x] Return 0 if `begin >= end`
  - [x] Get `new_algorithm_effective_cycle`
  - [x] Split computation at algorithm boundary
  - [x] Call `compute_old_reward` for cycles before boundary
  - [x] Call `compute_new_reward` for cycles after boundary
  - [x] Return sum of both

### 5.3 Old Reward Algorithm
- [x] Implement `compute_old_reward(storage: &EngineBackedEvmStateStore, begin: i64, end: i64, votes: &[DelegationVote]) -> Result<i64, String>`
  - [x] Iterate through each cycle from begin to end
  - [x] For each vote:
    - [x] Get `delegation_reward(cycle, witness)`
    - [x] Skip if reward <= 0
    - [x] Get `witness_vote(cycle, witness)`
    - [x] Skip if vote == REMARK or vote == 0
    - [x] Calculate `user_vote / total_vote * reward`
    - [x] Accumulate to total
  - [x] Return total reward

### 5.4 New Reward Algorithm (Vi-based)
- [x] Implement `compute_new_reward(storage: &EngineBackedEvmStateStore, begin: i64, end: i64, votes: &[DelegationVote]) -> Result<i64, String>`
  - [x] For each vote:
    - [x] Get `witness_vi(begin - 1, witness)` as BigInt
    - [x] Get `witness_vi(end - 1, witness)` as BigInt
    - [x] Calculate `delta_vi = end_vi - begin_vi`
    - [x] Skip if `delta_vi <= 0`
    - [x] Calculate `delta_vi * user_vote / DECIMAL_OF_VI_REWARD`
    - [x] Accumulate to total (convert BigInt to i64)
  - [x] Return total reward

### 5.5 Helper Functions
- [x] Implement `get_delegation_votes_from_account(storage: &EngineBackedEvmStateStore, address: &Address) -> Result<Vec<DelegationVote>, String>`
  - [x] Read account from storage using existing vote parsing
  - [x] Convert to DelegationVote format
  - [x] Return empty vec if no votes

---

## Phase 6: Integration with WithdrawBalance

### 6.1 Modify execute_withdraw_balance_contract
- [x] Open `rust-backend/crates/core/src/service/contracts/withdraw.rs`
- [x] Add import for delegation module
- [x] Before reading allowance, call `withdraw_reward()` via `compute_delegation_reward_if_enabled()`
  ```rust
  // Check if delegation reward computation is enabled
  // If enabled, compute delegation rewards and add to allowance
  let delegation_reward = self.compute_delegation_reward_if_enabled(storage_adapter, &owner_address)?;

  // Total allowance = base allowance + delegation reward
  let allowance = base_allowance.checked_add(delegation_reward)
      .ok_or("Overflow when adding delegation reward to allowance")?;
  ```
- [x] Update logging to show delegation reward
- [x] Delegation store changes are committed directly via storage_engine.put()

### 6.2 Configuration
- [x] Add config flag to `RemoteExecutionConfig`
  ```rust
  pub delegation_reward_enabled: bool,  // default: false for Phase 1
  ```
- [x] Gate delegation logic behind config flag
- [x] Add default to `Config::load()` and `RemoteExecutionConfig::default()`
- [x] Add to `config.toml`
  ```toml
  [execution.remote]
  delegation_reward_enabled = false
  ```

---

## Phase 7: gRPC Protocol (if needed)

### 7.1 Assess gRPC Requirements
- [x] Determine if delegation store is accessible via existing storage gRPC
  - **Result**: Yes, delegation store can be accessed via existing `Get`/`Put` gRPC methods with database name "delegation"
- [x] If separate database, add new gRPC methods
  - **Result**: Not needed - using existing gRPC methods with database routing

### 7.2 Add gRPC Methods (if needed)
- [x] Not needed - using existing `Get`/`Put` methods with database = "delegation"
- [x] Storage adapter accesses delegation store via `storage_engine.get()/put()` with "delegation" database name

---

## Phase 8: Testing

### 8.1 Unit Tests
- [x] Test key generation matches Java format (in `delegation/keys.rs`)
  - [x] `test_begin_cycle_key`
  - [x] `test_end_cycle_key`
  - [x] `test_account_vote_key`
  - [x] `test_reward_key`
  - [x] `test_witness_vote_key`
  - [x] `test_vi_key`
  - [x] `test_brokerage_key`
- [x] Test vote parsing from protobuf (in `delegation/types.rs`)
  - [x] `test_delegation_vote_creation`
  - [x] `test_account_vote_snapshot_serialization`
  - [x] `test_constants`
- [ ] Test reward computation (TODO - requires mock storage)
  - [ ] `test_compute_old_reward_single_cycle`
  - [ ] `test_compute_old_reward_multiple_cycles`
  - [ ] `test_compute_new_reward_single_cycle`
  - [ ] `test_compute_new_reward_multiple_cycles`
  - [ ] `test_compute_reward_algorithm_boundary`
- [ ] Test edge cases (TODO - requires mock storage)
  - [ ] `test_withdraw_reward_no_delegation_allowed`
  - [ ] `test_withdraw_reward_account_not_found`
  - [ ] `test_withdraw_reward_no_votes`
  - [ ] `test_withdraw_reward_same_cycle`
  - [ ] `test_withdraw_reward_zero_reward`

### 8.2 Integration Tests
- [ ] Create test with known delegation data
- [ ] Compare Rust output with Java output
- [ ] Test full WithdrawBalance flow with delegation

### 8.3 Regression Tests
- [ ] Run full blockchain replay (embedded mode)
- [ ] Run full blockchain replay (remote mode with delegation enabled)
- [ ] Compare CSV outputs
- [ ] Verify state_digest_sha256 matches
- [ ] Verify account_digest_sha256 matches

---

## Phase 9: Documentation

### 9.1 Code Documentation
- [x] Document all public functions in delegation module (doc comments added)
- [x] Document storage key formats (in `keys.rs` comments, referencing Java line numbers)
- [x] Document reward computation algorithms (in `delegation.rs` comments)

### 9.2 Update CLAUDE.md
- [ ] Add lesson about delegation store key formats
- [ ] Add lesson about Vi-based reward algorithm
- [ ] Document configuration options

---

## Phase 10: Rollout

### 10.1 Staged Rollout
- [ ] Deploy with `delegation_reward_enabled = false`
- [ ] Enable in test environment
- [ ] Run comparison tests
- [ ] Fix any discrepancies
- [ ] Enable in production with monitoring

### 10.2 Monitoring
- [ ] Add metrics for delegation reward computation time
- [ ] Add metrics for delegation store access count
- [ ] Add alerts for computation errors

---

## Verification Checklist

### Before Merge
- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] CSV parity achieved for WithdrawBalance transactions
- [ ] No regression in other contract types
- [ ] Code reviewed
- [ ] Documentation updated

### After Deployment
- [ ] Monitor error rates
- [ ] Verify delegation rewards match expected values
- [ ] Compare random samples with embedded execution
- [ ] Confirm no performance regression

---

## Files Changed Summary

| File | Action | Status |
|------|--------|--------|
| `crates/execution/src/delegation/mod.rs` | Create | ✅ Done |
| `crates/execution/src/delegation/keys.rs` | Create | ✅ Done |
| `crates/execution/src/delegation/types.rs` | Create | ✅ Done |
| `crates/execution/src/lib.rs` | Modify (add module) | ✅ Done |
| `crates/execution/src/storage_adapter/engine.rs` | Modify (add 15+ methods) | ✅ Done |
| `crates/execution/Cargo.toml` | Modify (add num-bigint) | ✅ Done |
| `crates/core/src/service/contracts/mod.rs` | Modify (add delegation module) | ✅ Done |
| `crates/core/src/service/contracts/withdraw.rs` | Modify | ✅ Done |
| `crates/core/src/service/contracts/delegation.rs` | Create | ✅ Done |
| `crates/core/Cargo.toml` | Modify (add num-bigint) | ✅ Done |
| `crates/common/src/config.rs` | Modify | ✅ Done |
| `config.toml` | Modify | ✅ Done |
| `proto/storage.proto` | Modify (if needed) | ⏭️ Not needed |

---

## Estimated Time

| Phase | Estimated Time |
|-------|----------------|
| Phase 1: Data Structures | 2-3 hours |
| Phase 2: Key Generation | 3-4 hours |
| Phase 3: Storage Read | 4-6 hours |
| Phase 4: Storage Write | 2-3 hours |
| Phase 5: Reward Logic | 6-8 hours |
| Phase 6: Integration | 2-3 hours |
| Phase 7: gRPC (if needed) | 2-4 hours |
| Phase 8: Testing | 8-12 hours |
| Phase 9: Documentation | 1-2 hours |
| Phase 10: Rollout | 2-4 hours |
| **Total** | **32-49 hours (4-6 days)** |

# Option 1: Full Rust Implementation - Detailed Checklist

## Overview
Implement complete `withdrawReward` logic in Rust, porting `MortgageService.withdrawReward()` from Java.

---

## Phase 1: Data Structures and Types

### 1.1 Define Core Types
- [ ] Create `rust-backend/crates/execution/src/delegation/mod.rs`
- [ ] Define `Vote` struct
  ```rust
  pub struct Vote {
      pub vote_address: Address,  // 20-byte witness address
      pub vote_count: i64,
  }
  ```
- [ ] Define `AccountVoteSnapshot` struct
  ```rust
  pub struct AccountVoteSnapshot {
      pub address: Address,
      pub votes: Vec<Vote>,
      pub allowance: i64,
  }
  ```
- [ ] Define constants
  ```rust
  pub const DELEGATION_STORE_REMARK: i64 = -1;
  pub const DECIMAL_OF_VI_REWARD: u128 = 1_000_000_000_000_000_000; // 10^18
  ```
- [ ] Add `num-bigint` crate to `Cargo.toml` for BigInt support
- [ ] Export module from `execution/src/lib.rs`

### 1.2 Implement Protobuf Parsing for Votes
- [ ] Add method to parse votes from Account protobuf
  ```rust
  fn parse_account_votes(account_bytes: &[u8]) -> Result<Vec<Vote>, String>
  ```
- [ ] Handle protobuf field 5 (repeated Vote votes) in Account message
- [ ] Test vote parsing with sample account data

---

## Phase 2: Storage Key Generation

### 2.1 Identify Java Key Formats
- [ ] Read `DelegationStore.java` to understand key formats
- [ ] Document key format for `begin_cycle`: prefix + address
- [ ] Document key format for `end_cycle`: prefix + address
- [ ] Document key format for `account_vote`: cycle (8 bytes BE) + address
- [ ] Document key format for `reward`: cycle (8 bytes BE) + witness
- [ ] Document key format for `witness_vote`: cycle (8 bytes BE) + witness
- [ ] Document key format for `witness_vi`: cycle (8 bytes BE) + witness
- [ ] Document key format for `brokerage`: cycle (8 bytes BE) + witness

### 2.2 Implement Key Generation in Rust
- [ ] Create `rust-backend/crates/execution/src/delegation/keys.rs`
- [ ] Implement `delegation_begin_cycle_key(address: &[u8]) -> Vec<u8>`
- [ ] Implement `delegation_end_cycle_key(address: &[u8]) -> Vec<u8>`
- [ ] Implement `delegation_account_vote_key(cycle: i64, address: &[u8]) -> Vec<u8>`
- [ ] Implement `delegation_reward_key(cycle: i64, witness: &[u8]) -> Vec<u8>`
- [ ] Implement `delegation_witness_vote_key(cycle: i64, witness: &[u8]) -> Vec<u8>`
- [ ] Implement `delegation_witness_vi_key(cycle: i64, witness: &[u8]) -> Vec<u8>`
- [ ] Implement `delegation_brokerage_key(cycle: i64, witness: &[u8]) -> Vec<u8>`
- [ ] Add unit tests comparing generated keys with Java

---

## Phase 3: Storage Adapter - Read Methods

### 3.1 Dynamic Properties Access
- [ ] Add `allow_change_delegation(&self) -> Result<bool, String>` to `EngineBackedEvmStateStore`
  - [ ] Read key `ALLOW_CHANGE_DELEGATION` from dynamic properties
  - [ ] Return false if not found (default)
- [ ] Add `get_current_cycle_number(&self) -> Result<i64, String>`
  - [ ] Read key `CURRENT_CYCLE_NUMBER` from dynamic properties
  - [ ] Parse as i64 (big-endian or varint, check Java format)
- [ ] Add `get_new_reward_algorithm_effective_cycle(&self) -> Result<i64, String>`
  - [ ] Read key `NEW_REWARD_ALGORITHM_EFFECTIVE_CYCLE`

### 3.2 Delegation Store Read Methods
- [ ] Add `get_delegation_begin_cycle(&self, address: &Address) -> Result<i64, String>`
  - [ ] Generate key using `delegation_begin_cycle_key`
  - [ ] Read from delegation store database
  - [ ] Parse as i64, default to 0 if not found
- [ ] Add `get_delegation_end_cycle(&self, address: &Address) -> Result<i64, String>`
  - [ ] Generate key using `delegation_end_cycle_key`
  - [ ] Read from delegation store database
  - [ ] Parse as i64, default to 0 if not found
- [ ] Add `get_account_vote(&self, cycle: i64, address: &Address) -> Result<Option<AccountVoteSnapshot>, String>`
  - [ ] Generate key using `delegation_account_vote_key`
  - [ ] Read from delegation store database
  - [ ] Parse Account protobuf, extract votes
  - [ ] Return None if not found
- [ ] Add `get_delegation_reward(&self, cycle: i64, witness: &Address) -> Result<i64, String>`
  - [ ] Generate key using `delegation_reward_key`
  - [ ] Read from delegation store database
  - [ ] Parse as i64, default to 0
- [ ] Add `get_witness_vote(&self, cycle: i64, witness: &Address) -> Result<i64, String>`
  - [ ] Generate key using `delegation_witness_vote_key`
  - [ ] Read from delegation store database
  - [ ] Parse as i64, handle REMARK value (-1)
- [ ] Add `get_witness_vi(&self, cycle: i64, witness: &Address) -> Result<BigInt, String>`
  - [ ] Generate key using `delegation_witness_vi_key`
  - [ ] Read from delegation store database
  - [ ] Parse as BigInt (check Java serialization format)
- [ ] Add `get_brokerage(&self, cycle: i64, witness: &Address) -> Result<i32, String>`
  - [ ] Generate key using `delegation_brokerage_key`
  - [ ] Read from delegation store database
  - [ ] Parse as i32, default to 20 (20% default brokerage)

### 3.3 Database Routing
- [ ] Identify delegation store database name in Rust storage service
- [ ] Add delegation store to database routing in storage adapter
- [ ] Ensure gRPC storage service can access delegation store
- [ ] Test basic read operations against delegation store

---

## Phase 4: Storage Adapter - Write Methods

### 4.1 Delegation Store Write Methods
- [ ] Add `set_delegation_begin_cycle(&mut self, address: &Address, cycle: i64) -> Result<(), String>`
  - [ ] Generate key using `delegation_begin_cycle_key`
  - [ ] Serialize cycle as i64 (match Java format)
  - [ ] Write to delegation store database
- [ ] Add `set_delegation_end_cycle(&mut self, address: &Address, cycle: i64) -> Result<(), String>`
  - [ ] Generate key using `delegation_end_cycle_key`
  - [ ] Serialize cycle as i64
  - [ ] Write to delegation store database
- [ ] Add `set_account_vote(&mut self, cycle: i64, address: &Address, snapshot: &AccountVoteSnapshot) -> Result<(), String>`
  - [ ] Generate key using `delegation_account_vote_key`
  - [ ] Serialize account snapshot to protobuf
  - [ ] Write to delegation store database

### 4.2 Track State Changes
- [ ] Add delegation store changes to `TronStateChange` enum
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
- [ ] Emit delegation changes for CSV parity (or gate behind config)

---

## Phase 5: Reward Computation - Core Logic

### 5.1 Main withdrawReward Function
- [ ] Create `rust-backend/crates/core/src/service/contracts/delegation.rs`
- [ ] Implement `withdraw_reward(storage: &mut Store, address: &Address) -> Result<i64, String>`
  - [ ] Check `allow_change_delegation()`, return 0 if false
  - [ ] Get account, return 0 if not found
  - [ ] Get `begin_cycle`, `end_cycle`, `current_cycle`
  - [ ] Return 0 if `begin_cycle > current_cycle`
  - [ ] Handle same-cycle check (begin == current)
  - [ ] Handle latest cycle reward withdrawal
  - [ ] Compute remaining cycle rewards
  - [ ] Update delegation store state
  - [ ] Return total computed reward

### 5.2 computeReward Function
- [ ] Implement `compute_reward(storage: &Store, begin: i64, end: i64, account: &AccountVoteSnapshot) -> Result<i64, String>`
  - [ ] Return 0 if `begin >= end`
  - [ ] Get `new_algorithm_effective_cycle`
  - [ ] Split computation at algorithm boundary
  - [ ] Call `compute_old_reward` for cycles before boundary
  - [ ] Call `compute_new_reward` for cycles after boundary
  - [ ] Return sum of both

### 5.3 Old Reward Algorithm
- [ ] Implement `compute_old_reward(storage: &Store, begin: i64, end: i64, votes: &[Vote]) -> Result<i64, String>`
  - [ ] Iterate through each cycle from begin to end
  - [ ] For each vote:
    - [ ] Get `delegation_reward(cycle, witness)`
    - [ ] Skip if reward <= 0
    - [ ] Get `witness_vote(cycle, witness)`
    - [ ] Skip if vote == REMARK or vote == 0
    - [ ] Calculate `user_vote / total_vote * reward`
    - [ ] Accumulate to total
  - [ ] Return total reward

### 5.4 New Reward Algorithm (Vi-based)
- [ ] Implement `compute_new_reward(storage: &Store, begin: i64, end: i64, votes: &[Vote]) -> Result<i64, String>`
  - [ ] For each vote:
    - [ ] Get `witness_vi(begin - 1, witness)` as BigInt
    - [ ] Get `witness_vi(end - 1, witness)` as BigInt
    - [ ] Calculate `delta_vi = end_vi - begin_vi`
    - [ ] Skip if `delta_vi <= 0`
    - [ ] Calculate `delta_vi * user_vote / DECIMAL_OF_VI_REWARD`
    - [ ] Accumulate to total (convert BigInt to i64)
  - [ ] Return total reward

### 5.5 Helper Functions
- [ ] Implement `get_account_votes(storage: &Store, address: &Address) -> Result<Vec<Vote>, String>`
  - [ ] Read account from storage
  - [ ] Parse votes from account protobuf
  - [ ] Return empty vec if no votes

---

## Phase 6: Integration with WithdrawBalance

### 6.1 Modify execute_withdraw_balance_contract
- [ ] Open `rust-backend/crates/core/src/service/contracts/withdraw.rs`
- [ ] Add import for delegation module
- [ ] Before reading allowance, call `withdraw_reward()`
  ```rust
  // Compute delegation reward and add to allowance
  let delegation_reward = self.withdraw_reward(storage_adapter, &owner_address)?;
  if delegation_reward > 0 {
      let mut account = storage_adapter.get_account(&owner_address)?
          .ok_or("Account not found")?;
      account.allowance += delegation_reward;
      storage_adapter.set_account(&owner_address, account)?;
  }
  ```
- [ ] Update logging to show delegation reward
- [ ] Ensure delegation store changes are committed

### 6.2 Configuration
- [ ] Add config flag to `RemoteExecutionConfig`
  ```rust
  pub delegation_reward_enabled: bool,  // default: false for Phase 1
  ```
- [ ] Gate delegation logic behind config flag
- [ ] Add to `config.toml`
  ```toml
  [remote_execution]
  delegation_reward_enabled = false
  ```

---

## Phase 7: gRPC Protocol (if needed)

### 7.1 Assess gRPC Requirements
- [ ] Determine if delegation store is accessible via existing storage gRPC
- [ ] If separate database, add new gRPC methods

### 7.2 Add gRPC Methods (if needed)
- [ ] Add to `proto/storage.proto`:
  - [ ] `GetDelegationValue(key) -> value`
  - [ ] `SetDelegationValue(key, value) -> success`
- [ ] Implement gRPC handlers in storage service
- [ ] Update storage adapter to use new gRPC methods

---

## Phase 8: Testing

### 8.1 Unit Tests
- [ ] Test key generation matches Java format
  - [ ] `test_delegation_begin_cycle_key`
  - [ ] `test_delegation_end_cycle_key`
  - [ ] `test_delegation_account_vote_key`
  - [ ] `test_delegation_reward_key`
  - [ ] `test_delegation_witness_vote_key`
  - [ ] `test_delegation_witness_vi_key`
- [ ] Test vote parsing from protobuf
  - [ ] `test_parse_account_votes_empty`
  - [ ] `test_parse_account_votes_single`
  - [ ] `test_parse_account_votes_multiple`
- [ ] Test reward computation
  - [ ] `test_compute_old_reward_single_cycle`
  - [ ] `test_compute_old_reward_multiple_cycles`
  - [ ] `test_compute_new_reward_single_cycle`
  - [ ] `test_compute_new_reward_multiple_cycles`
  - [ ] `test_compute_reward_algorithm_boundary`
- [ ] Test edge cases
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
- [ ] Document all public functions in delegation module
- [ ] Document storage key formats
- [ ] Document reward computation algorithms

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

| File | Action |
|------|--------|
| `crates/execution/src/delegation/mod.rs` | Create |
| `crates/execution/src/delegation/keys.rs` | Create |
| `crates/execution/src/delegation/types.rs` | Create |
| `crates/execution/src/lib.rs` | Modify (add module) |
| `crates/execution/src/storage_adapter/engine.rs` | Modify (add 15+ methods) |
| `crates/core/src/service/contracts/withdraw.rs` | Modify |
| `crates/core/src/service/contracts/delegation.rs` | Create |
| `crates/common/src/config.rs` | Modify |
| `config.toml` | Modify |
| `Cargo.toml` | Modify (add num-bigint) |
| `proto/storage.proto` | Modify (if needed) |

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

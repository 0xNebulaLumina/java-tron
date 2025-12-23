# Option 2: Java Pre-computes Delegation Reward - Detailed Checklist

## Overview
Java computes delegation reward using existing `MortgageService.queryReward()` and passes it to Rust via gRPC. Rust uses this pre-computed value. Java handles delegation state updates after execution.

---

## Phase 1: Protocol Buffer Changes

### 1.1 Update Java Proto
- [ ] Open `framework/src/main/proto/backend.proto`
- [ ] Add field to `ExecuteTransactionRequest`:
  ```protobuf
  message ExecuteTransactionRequest {
      // ... existing fields (1-19) ...

      // Pre-computed delegation reward for WithdrawBalanceContract
      // Java computes via MortgageService.queryReward() - account.allowance
      // Rust adds this to base allowance for correct withdrawal amount
      int64 pre_computed_delegation_reward = 20;
  }
  ```
- [ ] Regenerate Java protobuf classes
  ```bash
  ./gradlew :framework:generateProto
  ```
- [ ] Verify generated class has new field

### 1.2 Update Rust Proto
- [ ] Open `rust-backend/proto/backend.proto`
- [ ] Add same field to `ExecuteTransactionRequest`:
  ```protobuf
  int64 pre_computed_delegation_reward = 20;
  ```
- [ ] Regenerate Rust protobuf
  ```bash
  cd rust-backend && cargo build
  ```
- [ ] Verify generated struct has new field

---

## Phase 2: Java - Compute Delegation Reward

### 2.1 Add Helper Method
- [ ] Open `framework/src/main/java/org/tron/core/execution/spi/RemoteExecutionSPI.java`
- [ ] Add helper method to compute delegation-only reward:
  ```java
  /**
   * Compute delegation reward (excluding base allowance) for an address.
   * Uses queryReward() which returns total (delegation + allowance),
   * then subtracts current allowance to get delegation portion only.
   */
  private long computeDelegationReward(byte[] ownerAddress, TransactionContext context) {
      try {
          ChainBaseManager chainBaseManager = context.getStoreFactory().getChainBaseManager();
          MortgageService mortgageService = chainBaseManager.getMortgageService();
          AccountStore accountStore = chainBaseManager.getAccountStore();

          // Get current allowance from account
          AccountCapsule account = accountStore.get(ownerAddress);
          long currentAllowance = (account != null) ? account.getAllowance() : 0;

          // queryReward returns: delegation_reward + current_allowance
          long totalReward = mortgageService.queryReward(ownerAddress);

          // Delegation reward = total - current allowance
          long delegationReward = totalReward - currentAllowance;

          logger.debug("Computed delegation reward for {}: total={}, allowance={}, delegation={}",
              ByteArray.toHexString(ownerAddress), totalReward, currentAllowance, delegationReward);

          return Math.max(0, delegationReward); // Ensure non-negative
      } catch (Exception e) {
          logger.warn("Failed to compute delegation reward for {}: {}",
              ByteArray.toHexString(ownerAddress), e.getMessage());
          return 0;
      }
  }
  ```

### 2.2 Add Configuration Property
- [ ] Add JVM property check:
  ```java
  private static final boolean PRECOMPUTE_DELEGATION_ENABLED = Boolean.parseBoolean(
      System.getProperty("remote.exec.precompute.delegation.reward", "true"));
  ```
- [ ] Document property in startup scripts/config

### 2.3 Modify WithdrawBalanceContract Case
- [ ] Find the `switch` statement handling contract types (around line 438)
- [ ] Locate `case WithdrawBalanceContract:` block
- [ ] Add variable to track delegation reward:
  ```java
  long preComputedDelegationReward = 0;  // Will be set for WithdrawBalance
  ```
- [ ] In the `WithdrawBalanceContract` case, compute delegation reward:
  ```java
  case WithdrawBalanceContract:
      toAddress = new byte[0];
      data = new byte[0];
      txKind = TxKind.NON_VM;
      contractType = BackendOuterClass.ContractType.WITHDRAW_BALANCE_CONTRACT;

      // Compute delegation reward if enabled
      if (PRECOMPUTE_DELEGATION_ENABLED) {
          preComputedDelegationReward = computeDelegationReward(fromAddress, context);
          logger.info("WithdrawBalanceContract: pre-computed delegation reward = {} for {}",
              preComputedDelegationReward, ByteArray.toHexString(fromAddress));
      }
      break;
  ```

### 2.4 Add to gRPC Request
- [ ] Find where `ExecuteTransactionRequest` is built (search for `.build()`)
- [ ] Add the pre-computed delegation reward field:
  ```java
  ExecuteTransactionRequest.Builder requestBuilder = ExecuteTransactionRequest.newBuilder()
      .setFromAddress(ByteString.copyFrom(fromAddress))
      // ... other existing fields ...
      .setPreComputedDelegationReward(preComputedDelegationReward);  // NEW

  ExecuteTransactionRequest request = requestBuilder.build();
  ```
- [ ] Ensure field is set before `.build()` call

---

## Phase 3: Rust - Use Pre-computed Reward

### 3.1 Parse Pre-computed Reward from Request
- [ ] Open `rust-backend/crates/core/src/service/grpc.rs`
- [ ] Find the gRPC handler for `execute_transaction`
- [ ] Extract the pre-computed delegation reward from request:
  ```rust
  let pre_computed_delegation_reward = request.pre_computed_delegation_reward;
  ```
- [ ] Pass to contract execution method

### 3.2 Update Function Signature
- [ ] Open `rust-backend/crates/core/src/service/contracts/withdraw.rs`
- [ ] Modify function signature:
  ```rust
  pub(crate) fn execute_withdraw_balance_contract(
      &self,
      storage_adapter: &mut EngineBackedEvmStateStore,
      transaction: &TronTransaction,
      context: &TronExecutionContext,
      pre_computed_delegation_reward: i64,  // NEW parameter
  ) -> Result<TronExecutionResult, String>
  ```

### 3.3 Use Pre-computed Reward in Logic
- [ ] Find where allowance is read (around line 82):
  ```rust
  let base_allowance = storage_adapter.get_account_allowance(&owner_address)
      .map_err(|e| format!("Failed to read allowance: {}", e))?;
  ```
- [ ] Add total allowance calculation:
  ```rust
  // Calculate total allowance including pre-computed delegation reward
  let total_allowance = base_allowance + pre_computed_delegation_reward;

  info!("Account {} allowance breakdown: base={}, delegation={}, total={}",
        owner_tron, base_allowance, pre_computed_delegation_reward, total_allowance);
  ```
- [ ] Update validation to use total:
  ```rust
  if total_allowance <= 0 {
      warn!("Account {} has no reward to withdraw (base={}, delegation={})",
            owner_tron, base_allowance, pre_computed_delegation_reward);
      return Err("witnessAccount does not have any reward".to_string());
  }
  ```
- [ ] Update balance calculation to use total:
  ```rust
  let total_allowance_u64 = total_allowance as u64;
  let new_balance_u64 = old_balance_u64.checked_add(total_allowance_u64)
      .ok_or("Balance overflow when adding allowance")?;
  ```

### 3.4 Update WithdrawChange Emission
- [ ] Find where `WithdrawChange` is created (around line 123)
- [ ] Update amount to use total allowance:
  ```rust
  let withdraw_changes = vec![
      WithdrawChange {
          owner_address,
          amount: total_allowance,  // Use total, not just base
          latest_withdraw_time: now_ms,
      }
  ];
  ```

### 3.5 Add Configuration (Optional)
- [ ] Open `rust-backend/crates/common/src/config.rs`
- [ ] Add config flag:
  ```rust
  /// Whether to use pre-computed delegation reward from Java
  #[serde(default = "default_true")]
  pub use_precomputed_delegation_reward: bool,
  ```
- [ ] Add default function:
  ```rust
  fn default_true() -> bool { true }
  ```
- [ ] Update `config.toml`:
  ```toml
  [remote_execution]
  use_precomputed_delegation_reward = true
  ```

---

## Phase 4: Java - Apply Delegation State Updates

### 4.1 Modify applyWithdrawChanges
- [ ] Open `framework/src/main/java/org/tron/common/runtime/RuntimeSpiImpl.java`
- [ ] Find `applyWithdrawChanges` method (around line 589)
- [ ] Add MortgageService access:
  ```java
  MortgageService mortgageService = chainBaseManager.getMortgageService();
  ```

### 4.2 Call withdrawReward for State Updates
- [ ] Inside the for loop, before setting allowance to 0:
  ```java
  for (WithdrawChange withdrawChange : result.getWithdrawChanges()) {
      byte[] ownerAddress = withdrawChange.getOwnerAddress();

      // Call withdrawReward to update delegation store state
      // This updates: beginCycle, endCycle, accountVote
      // Note: This also adds delegation reward to allowance, but we'll reset it
      if (PRECOMPUTE_DELEGATION_ENABLED) {
          mortgageService.withdrawReward(ownerAddress);
          logger.debug("Called withdrawReward for delegation state update: {}",
              ByteArray.toHexString(ownerAddress));
      }

      // Re-read account after withdrawReward
      AccountCapsule accountCapsule = accountStore.get(ownerAddress);
      if (accountCapsule == null) {
          logger.warn("Account not found after withdrawReward: {}",
              ByteArray.toHexString(ownerAddress));
          continue;
      }

      // Reset allowance to 0 and update latestWithdrawTime
      // Balance was already updated by Rust's AccountChange
      accountCapsule.setAllowance(0);
      accountCapsule.setLatestWithdrawTime(withdrawChange.getLatestWithdrawTime());

      accountStore.put(accountCapsule.createDbKey(), accountCapsule);
      // ...
  }
  ```

### 4.3 Add Feature Flag Check
- [ ] Add flag to control behavior:
  ```java
  private static final boolean PRECOMPUTE_DELEGATION_ENABLED = Boolean.parseBoolean(
      System.getProperty("remote.exec.precompute.delegation.reward", "true"));
  ```
- [ ] Use flag to gate the withdrawReward call

---

## Phase 5: Update Call Sites

### 5.1 Update gRPC Handler Call
- [ ] Open `rust-backend/crates/core/src/service/grpc.rs`
- [ ] Find where `execute_withdraw_balance_contract` is called
- [ ] Update call to pass the pre-computed value:
  ```rust
  ContractType::WithdrawBalanceContract => {
      self.execute_withdraw_balance_contract(
          &mut storage_adapter,
          &transaction,
          &context,
          request.pre_computed_delegation_reward,  // Pass through
      )
  }
  ```

### 5.2 Update mod.rs (if method is called there)
- [ ] Check `rust-backend/crates/core/src/service/mod.rs`
- [ ] Update any calls to include new parameter

---

## Phase 6: Build and Compile

### 6.1 Build Java
- [ ] Clean and build:
  ```bash
  ./gradlew clean build -x test --dependency-verification=off
  ```
- [ ] Fix any compilation errors
- [ ] Verify proto classes regenerated

### 6.2 Build Rust
- [ ] Build Rust backend:
  ```bash
  cd rust-backend && cargo build --release
  ```
- [ ] Fix any compilation errors
- [ ] Verify proto structs have new field

---

## Phase 7: Testing

### 7.1 Unit Tests - Java
- [ ] Add test for `computeDelegationReward`:
  ```java
  @Test
  public void testComputeDelegationReward_withDelegation()
  @Test
  public void testComputeDelegationReward_noDelegation()
  @Test
  public void testComputeDelegationReward_accountNotFound()
  ```

### 7.2 Unit Tests - Rust
- [ ] Add test for pre-computed reward usage:
  ```rust
  #[test]
  fn test_withdraw_with_precomputed_delegation_reward()
  #[test]
  fn test_withdraw_with_zero_precomputed_reward()
  #[test]
  fn test_withdraw_with_negative_precomputed_reward()
  ```

### 7.3 Integration Tests
- [ ] Test WithdrawBalance with known delegation data
- [ ] Verify balance = old_balance + base_allowance + delegation_reward
- [ ] Verify delegation store state is updated
- [ ] Verify allowance is reset to 0

### 7.4 Regression Tests
- [ ] Run blockchain replay in embedded mode
  - [ ] Generate CSV: `embedded-embedded.csv`
- [ ] Run blockchain replay in remote mode
  - [ ] Generate CSV: `remote-remote.csv`
- [ ] Compare CSVs:
  ```bash
  python3 scripts/execution_csv_compare.py \
      output-directory/execution-csv/embedded-embedded.csv \
      output-directory/execution-csv/remote-remote.csv
  ```
- [ ] Verify no mismatches for WithdrawBalanceContract

### 7.5 Specific Transaction Test
- [ ] Test the specific failing transaction:
  - Block: 20161
  - TxId: bb378ccded4e0defb953a9d02fa071517e14d0cdf51fb533eea07618b843b7ff
  - Owner: TDGmmTC7xDgQGwH4FYRGuE7SFH2MePHYeH
- [ ] Verify balance difference is resolved (was 85 SUN)

---

## Phase 8: Edge Cases

### 8.1 Handle Edge Cases in Java
- [ ] Account doesn't exist → return 0
- [ ] MortgageService throws exception → catch, log, return 0
- [ ] queryReward returns 0 → delegation_reward = 0
- [ ] allowance > queryReward (shouldn't happen) → cap at 0

### 8.2 Handle Edge Cases in Rust
- [ ] pre_computed_delegation_reward is 0 → use base_allowance only
- [ ] pre_computed_delegation_reward is negative → treat as 0
- [ ] Overflow when adding to balance → return error

---

## Phase 9: Logging and Debugging

### 9.1 Java Logging
- [ ] Log delegation reward computation:
  ```java
  logger.info("WithdrawBalance pre-compute: owner={}, queryReward={}, allowance={}, delegation={}",
      address, queryReward, allowance, delegationReward);
  ```
- [ ] Log when sending to Rust:
  ```java
  logger.debug("Sending to Rust: preComputedDelegationReward={}", preComputedDelegationReward);
  ```
- [ ] Log after applying withdraw changes:
  ```java
  logger.info("Applied WithdrawChange: owner={}, amount={}, delegationStateUpdated={}",
      owner, amount, delegationStateUpdated);
  ```

### 9.2 Rust Logging
- [ ] Log received pre-computed value:
  ```rust
  info!("WithdrawBalance: received pre_computed_delegation_reward={}", reward);
  ```
- [ ] Log total allowance calculation:
  ```rust
  info!("WithdrawBalance: base={}, delegation={}, total={}", base, delegation, total);
  ```

---

## Phase 10: Documentation

### 10.1 Update CLAUDE.md
- [ ] Add lesson learned about delegation reward pre-computation
- [ ] Document the JVM property
- [ ] Document the data flow

### 10.2 Code Comments
- [ ] Add comments explaining the pre-computation flow
- [ ] Document why withdrawReward is called in applyWithdrawChanges
- [ ] Explain the queryReward - allowance calculation

---

## Phase 11: Rollout

### 11.1 Feature Flag Strategy
- [ ] Initial deployment: `remote.exec.precompute.delegation.reward=false`
- [ ] Test in staging: `remote.exec.precompute.delegation.reward=true`
- [ ] Production: Enable after staging validation

### 11.2 Monitoring
- [ ] Add metric for delegation reward computation
- [ ] Monitor WithdrawBalance transaction success rate
- [ ] Compare balance changes with expected values

---

## Verification Checklist

### Before Merge
- [ ] Java builds successfully
- [ ] Rust builds successfully
- [ ] Unit tests pass
- [ ] Integration tests pass
- [ ] CSV comparison shows no mismatches
- [ ] Code reviewed
- [ ] Logging is appropriate (not too verbose)

### After Deployment
- [ ] No errors in logs
- [ ] WithdrawBalance transactions succeed
- [ ] Delegation rewards are correct
- [ ] No regression in other contract types

---

## Files Changed Summary

| File | Action | Changes |
|------|--------|---------|
| `framework/src/main/proto/backend.proto` | Modify | Add `pre_computed_delegation_reward` field |
| `framework/src/main/java/.../RemoteExecutionSPI.java` | Modify | Add `computeDelegationReward()`, update request building |
| `framework/src/main/java/.../RuntimeSpiImpl.java` | Modify | Call `withdrawReward()` in `applyWithdrawChanges()` |
| `rust-backend/proto/backend.proto` | Modify | Add `pre_computed_delegation_reward` field |
| `rust-backend/crates/core/src/service/grpc.rs` | Modify | Parse and pass pre-computed value |
| `rust-backend/crates/core/src/service/contracts/withdraw.rs` | Modify | Use pre-computed value in calculation |
| `rust-backend/crates/common/src/config.rs` | Modify | Add config flag (optional) |
| `rust-backend/config.toml` | Modify | Add config flag (optional) |

---

## Estimated Time

| Phase | Estimated Time |
|-------|----------------|
| Phase 1: Proto Changes | 30 min |
| Phase 2: Java Compute | 1-2 hours |
| Phase 3: Rust Use | 1-2 hours |
| Phase 4: Java Apply | 1 hour |
| Phase 5: Call Sites | 30 min |
| Phase 6: Build | 30 min |
| Phase 7: Testing | 3-4 hours |
| Phase 8: Edge Cases | 1 hour |
| Phase 9: Logging | 30 min |
| Phase 10: Documentation | 30 min |
| Phase 11: Rollout | 1 hour |
| **Total** | **10-14 hours (1.5-2 days)** |

---

## Quick Reference: Key Changes

### Java - RemoteExecutionSPI.java
```java
// 1. Add helper method
private long computeDelegationReward(byte[] ownerAddress, TransactionContext context) { ... }

// 2. In WithdrawBalanceContract case
preComputedDelegationReward = computeDelegationReward(fromAddress, context);

// 3. When building request
.setPreComputedDelegationReward(preComputedDelegationReward)
```

### Java - RuntimeSpiImpl.java
```java
// In applyWithdrawChanges, before setting allowance to 0:
mortgageService.withdrawReward(ownerAddress);
```

### Rust - withdraw.rs
```rust
// Updated function signature
pub fn execute_withdraw_balance_contract(..., pre_computed_delegation_reward: i64) { ... }

// Use total allowance
let total_allowance = base_allowance + pre_computed_delegation_reward;
```

package org.tron.common.runtime;

import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import lombok.extern.slf4j.Slf4j;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.core.exception.ContractExeException;
import org.tron.core.exception.ContractValidateException;
import org.tron.core.ChainBaseManager;
import org.tron.core.execution.reporting.PreStateSnapshotRegistry;

import org.tron.core.execution.spi.ExecutionProgramResult;
import org.tron.core.execution.spi.ExecutionSPI;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;
import org.tron.core.execution.spi.ExecutionSpiFactory;
import org.tron.protos.Protocol.Transaction.Result.contractResult;
import org.tron.protos.Protocol.Account;
import org.tron.protos.Protocol.Vote;

import static org.tron.protos.contract.Common.ResourceCode.BANDWIDTH;
import static org.tron.protos.contract.Common.ResourceCode.ENERGY;

/**
 * ExecutionSPI-aware Runtime implementation that maintains the existing Runtime interface while
 * delegating execution to the configured ExecutionSPI implementation (EMBEDDED, REMOTE, or SHADOW).
 *
 * <p>This class provides backward compatibility by using ExecutionProgramResult, which extends
 * ProgramResult, eliminating the need for type conversion.
 */
@Slf4j(topic = "VM")
public class RuntimeSpiImpl implements Runtime {

  private final ExecutionSPI executionSPI;
  private TransactionContext context;
  private ExecutionProgramResult executionResult;
  private String runtimeError;



  /**
   * Constructor that ensures ExecutionSPI factory is properly initialized.
   * The execution mode is determined dynamically from configuration sources during factory initialization.
   * This maintains the singleton pattern for efficiency while supporting dynamic configuration.
   */
  public RuntimeSpiImpl() {
    // // Ensure factory is initialized (this will determine execution mode from configuration)
    // ExecutionSpiFactory.initialize();

    this.executionSPI = ExecutionSpiFactory.getInstance();
    if (this.executionSPI == null) {
      throw new RuntimeException(
          "ExecutionSPI not initialized. Call ExecutionSpiFactory.initialize() first.");
    }
    logger.info(
        "RuntimeSpiImpl initialized with execution mode: {}",
        ExecutionSpiFactory.determineExecutionMode());
  }

  @Override
  public void execute(TransactionContext context)
      throws ContractValidateException, ContractExeException {
    this.context = context;

    try {
      logger.debug(
          "Executing transaction with ExecutionSPI: {}", context.getTrxCap().getTransactionId());

      // Use ExecutionSPI for execution
      CompletableFuture<ExecutionProgramResult> future =
          executionSPI.executeTransaction(context);
      this.executionResult = future.get(); // Synchronous execution

      // Store runtime error if execution failed
      if (!executionResult.isSuccess()) {
        this.runtimeError = executionResult.getErrorMessage();
      }

      // Capture pre-state snapshot for CSV reporting (before applying changes)
      capturePreStateSnapshot(executionResult, context);

      // Apply state changes to local database for remote execution
      applyStateChangesToLocalDatabase(executionResult, context);

      // Apply freeze ledger changes to local database (Phase 2)
      applyFreezeLedgerChanges(executionResult, context);

      // Apply TRC-10 changes to local database (Phase 2)
      applyTrc10Changes(executionResult, context);

      // Apply Vote changes to local database (Phase 2)
      applyVoteChanges(executionResult, context);

      // Apply Withdraw changes to local database (WithdrawBalanceContract)
      applyWithdrawChanges(executionResult, context);

      // Since ExecutionProgramResult extends ProgramResult, we can use it directly
      context.setProgramResult(executionResult);

      logger.debug(
          "ExecutionSPI execution completed. Success: {}, Energy used: {}, State changes applied: {}",
          executionResult.isSuccess(),
          executionResult.getEnergyUsed(),
          executionResult.getStateChanges() != null ? executionResult.getStateChanges().size() : 0);

    } catch (Exception e) {
      logger.error(
          "ExecutionSPI execution failed for transaction: {}",
          context.getTrxCap().getTransactionId(),
          e);

      // Create a failed ExecutionProgramResult for compatibility
      this.executionResult = createFailedExecutionProgramResult(e.getMessage());
      context.setProgramResult(executionResult);
      this.runtimeError = e.getMessage();

      throw new ContractExeException("Execution failed: " + e.getMessage());
    }
  }

  @Override
  public ProgramResult getResult() {
    if (context == null) {
      return ProgramResult.createEmpty();
    }
    return context.getProgramResult();
  }

  @Override
  public String getRuntimeError() {
    return runtimeError;
  }



  /** Create a failed ExecutionProgramResult when ExecutionSPI execution fails. */
  private ExecutionProgramResult createFailedExecutionProgramResult(String errorMessage) {
    ExecutionProgramResult result = new ExecutionProgramResult();

    // Set failure state
    result.setResultCode(contractResult.REVERT);
    result.setRevert();
    result.setRuntimeError(errorMessage);
    result.setException(new RuntimeException(errorMessage));

    logger.debug("Created failed ExecutionProgramResult with error: {}", errorMessage);
    return result;
  }

  /**
   * Apply state changes from remote execution to the local Java-Tron database.
   * This is critical for remote execution mode to ensure local state consistency.
   */
  private void applyStateChangesToLocalDatabase(ExecutionProgramResult result, TransactionContext context) {
    if (result.getStateChanges() == null || result.getStateChanges().isEmpty()) {
      logger.debug("No state changes to apply for transaction: {}", 
          context.getTrxCap().getTransactionId());
      return;
    }

    logger.info("Applying {} state changes to local database for transaction: {}", 
        result.getStateChanges().size(), context.getTrxCap().getTransactionId());

    try {
      // Get the chain base manager from context
      ChainBaseManager chainBaseManager = context.getStoreFactory().getChainBaseManager();
      
      for (ExecutionSPI.StateChange stateChange : result.getStateChanges()) {
        applyStateChange(stateChange, chainBaseManager, context);
      }
      
      logger.info("Successfully applied {} state changes for transaction: {}", 
          result.getStateChanges().size(), context.getTrxCap().getTransactionId());
          
    } catch (Exception e) {
      logger.error("Failed to apply state changes for transaction: {}, error: {}", 
          context.getTrxCap().getTransactionId(), e.getMessage(), e);
      // Don't throw exception here as it would break the transaction flow
      // The transaction might still be valid even if state sync fails
    }
  }

  /**
   * Apply freeze ledger changes from remote execution to the local Java-Tron database (Phase 2).
   * This ensures BandwidthProcessor sees updated netLimit for subsequent transactions in the same block.
   * Can be disabled via JVM property: -Dremote.exec.apply.freeze=false for rapid rollback.
   */
  private void applyFreezeLedgerChanges(ExecutionProgramResult result, TransactionContext context) {
    // Check JVM toggle for rapid rollback
    // Default is true (apply enabled), can be disabled with -Dremote.exec.apply.freeze=false
    boolean applyEnabled = Boolean.parseBoolean(
        System.getProperty("remote.exec.apply.freeze", "true"));

    if (!applyEnabled) {
      logger.debug("Freeze ledger changes application disabled by JVM property " +
          "(-Dremote.exec.apply.freeze=false) for transaction: {}",
          context.getTrxCap().getTransactionId());
      return;
    }

    // Check for freeze changes
    boolean hasFreezeChanges = result.getFreezeChanges() != null && !result.getFreezeChanges().isEmpty();
    boolean hasGlobalChanges = result.getGlobalResourceChanges() != null && !result.getGlobalResourceChanges().isEmpty();

    if (!hasFreezeChanges && !hasGlobalChanges) {
      logger.debug("No freeze ledger changes to apply for transaction: {}",
          context.getTrxCap().getTransactionId());
      return;
    }

    logger.info("Applying freeze ledger changes to local database for transaction: {} (freeze={}, global={})",
        context.getTrxCap().getTransactionId(), result.getFreezeChanges().size(),
        result.getGlobalResourceChanges().size());

    try {
      ChainBaseManager chainBaseManager = context.getStoreFactory().getChainBaseManager();

      // Apply freeze changes to account store
      if (hasFreezeChanges) {
        for (ExecutionSPI.FreezeLedgerChange freezeChange : result.getFreezeChanges()) {
          applyFreezeLedgerChange(freezeChange, chainBaseManager, context);
        }
      }

      // Apply global resource totals to dynamic properties store
      if (hasGlobalChanges) {
        for (ExecutionSPI.GlobalResourceTotalsChange globalChange : result.getGlobalResourceChanges()) {
          applyGlobalResourceChange(globalChange, chainBaseManager, context);
        }
      }

      logger.info("Successfully applied freeze ledger changes for transaction: {}",
          context.getTrxCap().getTransactionId());

    } catch (Exception e) {
      logger.error("Failed to apply freeze ledger changes for transaction: {}, error: {}",
          context.getTrxCap().getTransactionId(), e.getMessage(), e);
      // Don't throw exception - maintain transaction flow
    }
  }

  /**
   * Apply a single freeze ledger change to an account.
   */
  private void applyFreezeLedgerChange(ExecutionSPI.FreezeLedgerChange freezeChange,
                                      ChainBaseManager chainBaseManager,
                                      TransactionContext context) {
    try {
      byte[] ownerAddress = freezeChange.getOwnerAddress();
      String addressStr = org.tron.common.utils.StringUtil.encode58Check(ownerAddress);

      // Get or create account
      AccountCapsule accountCapsule = chainBaseManager.getAccountStore().get(ownerAddress);
      if (accountCapsule == null) {
        logger.warn("Account not found for freeze change, creating: {}", addressStr);
        // Create new account
        Account.Builder accountBuilder = Account.newBuilder()
            .setAddress(com.google.protobuf.ByteString.copyFrom(ownerAddress))
            .setBalance(0)
            .setCreateTime(System.currentTimeMillis())
            .setType(org.tron.protos.Protocol.AccountType.Normal);
        accountCapsule = new AccountCapsule(accountBuilder.build());
      }

      // Apply freeze change based on v2_model flag
      if (freezeChange.isV2Model()) {
        // V2 model: Update FrozenV2 list
        applyFreezeV2Change(accountCapsule, freezeChange, addressStr);
      } else {
        // V1 model: Update Frozen field
        applyFreezeV1Change(accountCapsule, freezeChange, addressStr);
      }

      // Store the updated account
      chainBaseManager.getAccountStore().put(ownerAddress, accountCapsule);

      // Mark account as dirty for resource processor synchronization
      org.tron.core.storage.sync.ResourceSyncContext.recordAccountDirty(ownerAddress);

      logger.debug("Applied freeze change: owner={}, resource={}, amount={}, expiration={}, v2={}",
          addressStr, freezeChange.getResource(), freezeChange.getAmount(),
          freezeChange.getExpirationMs(), freezeChange.isV2Model());

    } catch (Exception e) {
      logger.error("Failed to apply freeze ledger change for address: {}, error: {}",
          org.tron.common.utils.StringUtil.encode58Check(freezeChange.getOwnerAddress()),
          e.getMessage(), e);
    }
  }

  /**
   * Apply V1 freeze change to account's Frozen field.
   */
  private void applyFreezeV1Change(AccountCapsule accountCapsule,
                                  ExecutionSPI.FreezeLedgerChange freezeChange,
                                  String addressStr) {
    long amount = freezeChange.getAmount();
    long expirationMs = freezeChange.getExpirationMs();

    // Map resource type to Tron protocol resource code
    switch (freezeChange.getResource()) {
      case BANDWIDTH:
        // Update frozen balance for bandwidth
        accountCapsule.setFrozenForBandwidth(amount, expirationMs);
        logger.debug("Updated V1 frozen bandwidth for {}: amount={}, expiration={}",
            addressStr, amount, expirationMs);
        break;

      case ENERGY:
        // Update frozen balance for energy
        accountCapsule.setFrozenForEnergy(amount, expirationMs);
        logger.debug("Updated V1 frozen energy for {}: amount={}, expiration={}",
            addressStr, amount, expirationMs);
        break;

      case TRON_POWER:
        // Tron Power is typically tied to bandwidth in V1
        logger.warn("TRON_POWER resource in V1 model not directly supported, treating as BANDWIDTH");
        accountCapsule.setFrozenForBandwidth(amount, expirationMs);
        break;

      default:
        logger.warn("Unknown resource type for V1 freeze: {}", freezeChange.getResource());
    }
  }

  /**
   * Apply V2 freeze change to account's FrozenV2 list.
   * Uses absolute semantics: sets the frozen amount to the exact value, not a delta.
   * This ensures idempotency - applying the same change multiple times yields the same result.
   */
  private void applyFreezeV2Change(AccountCapsule accountCapsule,
                                  ExecutionSPI.FreezeLedgerChange freezeChange,
                                  String addressStr) {
    long amount = freezeChange.getAmount();
    org.tron.protos.contract.Common.ResourceCode resourceType;

    // Map resource type to corresponding protobuf ResourceCode
    switch (freezeChange.getResource()) {
      case BANDWIDTH:
        resourceType = org.tron.protos.contract.Common.ResourceCode.BANDWIDTH;
        break;
      case ENERGY:
        resourceType = org.tron.protos.contract.Common.ResourceCode.ENERGY;
        break;
      case TRON_POWER:
        resourceType = org.tron.protos.contract.Common.ResourceCode.TRON_POWER;
        break;
      default:
        logger.warn("Unknown resource type for V2 freeze: {}", freezeChange.getResource());
        return;
    }

    // Get current FrozenV2 list and find existing entry for this resource
    java.util.List<org.tron.protos.Protocol.Account.FreezeV2> frozenV2List =
        accountCapsule.getFrozenV2List();
    int existingIndex = -1;

    for (int i = 0; i < frozenV2List.size(); i++) {
      if (frozenV2List.get(i).getType().equals(resourceType)) {
        existingIndex = i;
        break;
      }
    }

    if (amount == 0) {
      // Amount is zero - remove the entry if it exists
      if (existingIndex >= 0) {
        // Remove by rebuilding the list without this entry
        org.tron.protos.Protocol.Account.Builder accountBuilder =
            accountCapsule.getInstance().toBuilder();
        accountBuilder.clearFrozenV2();
        for (int i = 0; i < frozenV2List.size(); i++) {
          if (i != existingIndex) {
            accountBuilder.addFrozenV2(frozenV2List.get(i));
          }
        }
        // Update the account capsule with the rebuilt account
        accountCapsule.setInstance(accountBuilder.build());
        logger.debug("Removed V2 frozen {} for {} (amount=0)", resourceType, addressStr);
      } else {
        logger.debug("No existing V2 frozen {} entry to remove for {}", resourceType, addressStr);
      }
    } else {
      // Amount is non-zero - set absolute value
      org.tron.protos.Protocol.Account.FreezeV2 newFreezeV2 =
          org.tron.protos.Protocol.Account.FreezeV2.newBuilder()
              .setType(resourceType)
              .setAmount(amount)
              .build();

      if (existingIndex >= 0) {
        // Update existing entry with absolute amount
        accountCapsule.updateFrozenV2List(existingIndex, newFreezeV2);
        logger.debug("Updated V2 frozen {} for {} to absolute amount: {} (was at index {})",
            resourceType, addressStr, amount, existingIndex);
      } else {
        // Add new entry with absolute amount
        accountCapsule.addFrozenV2List(newFreezeV2);
        logger.debug("Added new V2 frozen {} for {} with absolute amount: {}",
            resourceType, addressStr, amount);
      }
    }
  }

  /**
   * Apply global resource totals change to DynamicPropertiesStore.
   */
  private void applyGlobalResourceChange(ExecutionSPI.GlobalResourceTotalsChange globalChange,
                                        ChainBaseManager chainBaseManager,
                                        TransactionContext context) {
    try {
      org.tron.core.store.DynamicPropertiesStore dynamicStore =
          chainBaseManager.getDynamicPropertiesStore();

      if (dynamicStore == null) {
        logger.warn("DynamicPropertiesStore not available for global resource change");
        return;
      }

      // Update global totals
      dynamicStore.saveTotalNetWeight(globalChange.getTotalNetWeight());
      org.tron.core.storage.sync.ResourceSyncContext.recordDynamicKeyDirty("TOTAL_NET_WEIGHT".getBytes());

      dynamicStore.saveTotalNetLimit(globalChange.getTotalNetLimit());
      org.tron.core.storage.sync.ResourceSyncContext.recordDynamicKeyDirty("TOTAL_NET_LIMIT".getBytes());

      dynamicStore.saveTotalEnergyWeight(globalChange.getTotalEnergyWeight());
      org.tron.core.storage.sync.ResourceSyncContext.recordDynamicKeyDirty("TOTAL_ENERGY_WEIGHT".getBytes());

      dynamicStore.saveTotalEnergyCurrentLimit(globalChange.getTotalEnergyLimit());
      org.tron.core.storage.sync.ResourceSyncContext.recordDynamicKeyDirty("TOTAL_ENERGY_CURRENT_LIMIT".getBytes());

      logger.info("Applied global resource change: netWeight={}, netLimit={}, energyWeight={}, energyLimit={}",
          globalChange.getTotalNetWeight(), globalChange.getTotalNetLimit(),
          globalChange.getTotalEnergyWeight(), globalChange.getTotalEnergyLimit());

    } catch (Exception e) {
      logger.error("Failed to apply global resource change, error: {}", e.getMessage(), e);
    }
  }

  /**
   * Apply TRC-10 changes to local database (Phase 2: full TRC-10 ledger semantics).
   * Creates AssetIssueCapsule entries and updates issuer account asset maps.
   */
  private void applyTrc10Changes(ExecutionProgramResult result, TransactionContext context) {
    // Check JVM toggle for rapid rollback
    // Default is true (apply enabled), can be disabled with -Dremote.exec.apply.trc10=false
    boolean applyEnabled = Boolean.parseBoolean(
        System.getProperty("remote.exec.apply.trc10", "true"));

    if (!applyEnabled) {
      logger.debug("TRC-10 changes application disabled by JVM property " +
          "(-Dremote.exec.apply.trc10=false) for transaction: {}",
          context.getTrxCap().getTransactionId());
      return;
    }

    // Check for TRC-10 changes
    if (result.getTrc10Changes() == null || result.getTrc10Changes().isEmpty()) {
      logger.debug("No TRC-10 changes to apply for transaction: {}",
          context.getTrxCap().getTransactionId());
      return;
    }

    logger.info("Applying {} TRC-10 changes to local database for transaction: {}",
        result.getTrc10Changes().size(), context.getTrxCap().getTransactionId());

    try {
      ChainBaseManager chainBaseManager = context.getStoreFactory().getChainBaseManager();

      // Apply TRC-10 changes
      for (ExecutionSPI.Trc10Change trc10Change : result.getTrc10Changes()) {
        if (trc10Change.hasAssetIssued()) {
          applyAssetIssuedChange(trc10Change.getAssetIssued(), chainBaseManager, context);
        } else if (trc10Change.hasAssetTransferred()) {
          applyAssetTransferredChange(trc10Change.getAssetTransferred(), chainBaseManager, context);
        }
      }

      logger.info("Successfully applied TRC-10 changes for transaction: {}",
          context.getTrxCap().getTransactionId());

    } catch (Exception e) {
      logger.error("Failed to apply TRC-10 changes for transaction: {}, error: {}",
          context.getTrxCap().getTransactionId(), e.getMessage(), e);
      // Don't throw exception - maintain transaction flow
    }
  }

  /**
   * Apply VoteChanges to Account.votes to maintain parity with embedded mode.
   * Toggle with -Dremote.exec.apply.vote=false to disable if needed.
   */
  private void applyVoteChanges(ExecutionProgramResult result, TransactionContext context) {
    // JVM gate: default true
    boolean applyEnabled = Boolean.parseBoolean(
        System.getProperty("remote.exec.apply.vote", "true"));

    if (!applyEnabled) {
      logger.debug("Vote changes application disabled by JVM property (-Dremote.exec.apply.vote=false) for transaction: {}",
          context.getTrxCap().getTransactionId());
      return;
    }

    if (result.getVoteChanges() == null || result.getVoteChanges().isEmpty()) {
      logger.debug("No VoteChanges to apply for transaction: {}",
          context.getTrxCap().getTransactionId());
      return;
    }

    try {
      org.tron.core.ChainBaseManager chainBaseManager = context.getStoreFactory()
          .getChainBaseManager();
      org.tron.core.store.AccountStore accountStore = chainBaseManager.getAccountStore();
      org.tron.core.store.VotesStore votesStore = chainBaseManager.getVotesStore();

      if (accountStore == null) {
        logger.warn("AccountStore not available, cannot apply VoteChanges");
        return;
      }
      if (votesStore == null) {
        logger.warn("VotesStore not available, cannot apply VoteChanges");
        return;
      }

      for (org.tron.core.execution.spi.ExecutionSPI.VoteChange voteChange : result.getVoteChanges()) {
        byte[] ownerAddress = voteChange.getOwnerAddress();
        org.tron.core.capsule.AccountCapsule accountCapsule = accountStore.get(ownerAddress);

        if (accountCapsule == null) {
          logger.warn("Account not found for VoteChange: {}",
              org.tron.common.utils.ByteArray.toHexString(ownerAddress));
          continue;
        }

        // Get or create VotesCapsule (matches VoteWitnessActuator pattern)
        org.tron.core.capsule.VotesCapsule votesCapsule;
        if (!votesStore.has(ownerAddress)) {
          votesCapsule = new org.tron.core.capsule.VotesCapsule(
              com.google.protobuf.ByteString.copyFrom(ownerAddress),
              accountCapsule.getVotesList());
        } else {
          votesCapsule = votesStore.get(ownerAddress);
        }

        // Clear old votes from both capsules
        accountCapsule.clearVotes();
        votesCapsule.clearNewVotes();

        // Add new votes to both capsules
        for (org.tron.core.execution.spi.ExecutionSPI.VoteEntry voteEntry : voteChange.getVotes()) {
          com.google.protobuf.ByteString voteAddress =
              com.google.protobuf.ByteString.copyFrom(voteEntry.getVoteAddress());
          accountCapsule.addVotes(voteAddress, voteEntry.getVoteCount());
          votesCapsule.addNewVotes(voteAddress, voteEntry.getVoteCount());
        }

        // Persist to both stores (matches VoteWitnessActuator pattern)
        accountStore.put(accountCapsule.createDbKey(), accountCapsule);
        votesStore.put(ownerAddress, votesCapsule);

        // Mark dirty for resource sync
        org.tron.core.storage.sync.ResourceSyncContext.recordAccountDirty(ownerAddress);

        logger.debug("Applied VoteChange to Account.votes and VotesStore: owner={}, votes={}",
            org.tron.common.utils.ByteArray.toHexString(ownerAddress),
            voteChange.getVotes().size());
      }

      logger.info("Successfully applied {} VoteChanges for transaction: {}",
          result.getVoteChanges().size(), context.getTrxCap().getTransactionId());

    } catch (Exception e) {
      logger.error("Failed to apply VoteChanges for transaction: {}, error: {}",
          context.getTrxCap().getTransactionId(), e.getMessage(), e);
    }
  }

  /**
   * Apply WithdrawChanges to set Account.allowance=0 and Account.latestWithdrawTime.
   * Balance delta is already applied by AccountChange; this handles the allowance/time reset.
   * Toggle with -Dremote.exec.apply.withdraw=false to disable if needed.
   */
  private void applyWithdrawChanges(ExecutionProgramResult result, TransactionContext context) {
    // JVM gate: default true
    boolean applyEnabled = Boolean.parseBoolean(
        System.getProperty("remote.exec.apply.withdraw", "true"));

    if (!applyEnabled) {
      logger.debug("Withdraw changes application disabled by JVM property (-Dremote.exec.apply.withdraw=false) for transaction: {}",
          context.getTrxCap().getTransactionId());
      return;
    }

    if (result.getWithdrawChanges() == null || result.getWithdrawChanges().isEmpty()) {
      logger.debug("No WithdrawChanges to apply for transaction: {}",
          context.getTrxCap().getTransactionId());
      return;
    }

    try {
      org.tron.core.ChainBaseManager chainBaseManager = context.getStoreFactory()
          .getChainBaseManager();
      org.tron.core.store.AccountStore accountStore = chainBaseManager.getAccountStore();

      if (accountStore == null) {
        logger.warn("AccountStore not available, cannot apply WithdrawChanges");
        return;
      }

      for (org.tron.core.execution.spi.ExecutionSPI.WithdrawChange withdrawChange : result.getWithdrawChanges()) {
        byte[] ownerAddress = withdrawChange.getOwnerAddress();
        org.tron.core.capsule.AccountCapsule accountCapsule = accountStore.get(ownerAddress);

        if (accountCapsule == null) {
          logger.warn("Account not found for WithdrawChange: {}",
              org.tron.common.utils.ByteArray.toHexString(ownerAddress));
          continue;
        }

        // Get the withdrawn amount for logging
        long withdrawnAmount = withdrawChange.getAmount();
        long latestWithdrawTime = withdrawChange.getLatestWithdrawTime();

        // Set allowance to 0 and update latestWithdrawTime
        // Note: Balance delta is already applied by AccountChange from Rust
        accountCapsule.setAllowance(0);
        accountCapsule.setLatestWithdrawTime(latestWithdrawTime);

        // Persist the updated account
        accountStore.put(accountCapsule.createDbKey(), accountCapsule);

        // Mark dirty for resource sync
        org.tron.core.storage.sync.ResourceSyncContext.recordAccountDirty(ownerAddress);

        logger.debug("Applied WithdrawChange: owner={}, amount={}, latestWithdrawTime={}, allowance set to 0",
            org.tron.common.utils.ByteArray.toHexString(ownerAddress),
            withdrawnAmount,
            latestWithdrawTime);
      }

      logger.info("Successfully applied {} WithdrawChanges for transaction: {}",
          result.getWithdrawChanges().size(), context.getTrxCap().getTransactionId());

    } catch (Exception e) {
      logger.error("Failed to apply WithdrawChanges for transaction: {}, error: {}",
          context.getTrxCap().getTransactionId(), e.getMessage(), e);
    }
  }

  /**
   * Apply a single AssetIssued change to create AssetIssueCapsule and update account.
   */
  private void applyAssetIssuedChange(ExecutionSPI.Trc10AssetIssued assetIssued,
                                     ChainBaseManager chainBaseManager,
                                     TransactionContext context) {
    try {
      byte[] ownerAddress = assetIssued.getOwnerAddress();
      String addressStr = org.tron.common.utils.StringUtil.encode58Check(ownerAddress);
      String name = new String(assetIssued.getName(), java.nio.charset.StandardCharsets.UTF_8);

      logger.info("Applying AssetIssued change: owner={}, name={}, totalSupply={}, precision={}",
          addressStr, name, assetIssued.getTotalSupply(), assetIssued.getPrecision());

      // Get stores
      org.tron.core.store.DynamicPropertiesStore dynamicStore =
          chainBaseManager.getDynamicPropertiesStore();
      org.tron.core.store.AssetIssueStore assetIssueStore =
          chainBaseManager.getAssetIssueStore();
      org.tron.core.store.AssetIssueV2Store assetIssueV2Store =
          chainBaseManager.getAssetIssueV2Store();
      org.tron.core.store.AccountStore accountStore =
          chainBaseManager.getAccountStore();

      // 1. Determine or compute token ID
      String tokenId = assetIssued.getTokenId();
      if (tokenId == null || tokenId.isEmpty()) {
        // Java needs to compute TOKEN_ID_NUM
        long currentTokenId = dynamicStore.getTokenIdNum();
        long newTokenId = currentTokenId + 1;
        tokenId = String.valueOf(newTokenId);

        // Save incremented TOKEN_ID_NUM
        dynamicStore.saveTokenIdNum(newTokenId);
        org.tron.core.storage.sync.ResourceSyncContext.recordDynamicKeyDirty("TOKEN_ID_NUM".getBytes());

        logger.debug("Computed new token ID: {} (previous: {})", newTokenId, currentTokenId);
      }

      // 2. Check ALLOW_SAME_TOKEN_NAME to determine V1 vs V2 behavior
      long allowSameTokenName = dynamicStore.getAllowSameTokenName();
      boolean createV1 = (allowSameTokenName == 0); // Legacy mode: create V1 entry

      // 3. Create AssetIssueCapsule
      org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract.Builder contractBuilder =
          org.tron.protos.contract.AssetIssueContractOuterClass.AssetIssueContract.newBuilder()
          .setOwnerAddress(com.google.protobuf.ByteString.copyFrom(ownerAddress))
          .setName(com.google.protobuf.ByteString.copyFrom(assetIssued.getName()))
          .setAbbr(com.google.protobuf.ByteString.copyFrom(assetIssued.getAbbr()))
          .setTotalSupply(assetIssued.getTotalSupply())
          .setTrxNum(assetIssued.getTrxNum())
          .setPrecision(assetIssued.getPrecision())
          .setNum(assetIssued.getNum())
          .setStartTime(assetIssued.getStartTime())
          .setEndTime(assetIssued.getEndTime())
          .setDescription(com.google.protobuf.ByteString.copyFrom(assetIssued.getDescription()))
          .setUrl(com.google.protobuf.ByteString.copyFrom(assetIssued.getUrl()))
          .setFreeAssetNetLimit(assetIssued.getFreeAssetNetLimit())
          .setPublicFreeAssetNetLimit(assetIssued.getPublicFreeAssetNetLimit())
          .setPublicFreeAssetNetUsage(assetIssued.getPublicFreeAssetNetUsage())
          .setPublicLatestFreeNetTime(assetIssued.getPublicLatestFreeNetTime())
          .setId(tokenId);

      org.tron.core.capsule.AssetIssueCapsule assetIssueCapsule =
          new org.tron.core.capsule.AssetIssueCapsule(contractBuilder.build());

      // 4. Store AssetIssue entries
      if (createV1) {
        // V1: Store by name
        assetIssueStore.put(assetIssued.getName(), assetIssueCapsule);
        logger.debug("Created AssetIssue V1 entry with name: {}", name);
      }

      // V2: Always store by token ID
      assetIssueV2Store.put(tokenId.getBytes(), assetIssueCapsule);
      logger.debug("Created AssetIssue V2 entry with token ID: {}", tokenId);

      // 5. Update issuer account asset maps
      org.tron.core.capsule.AccountCapsule issuerAccount = accountStore.get(ownerAddress);
      if (issuerAccount == null) {
        logger.warn("Issuer account not found, creating: {}", addressStr);
        org.tron.protos.Protocol.Account.Builder accountBuilder =
            org.tron.protos.Protocol.Account.newBuilder()
            .setAddress(com.google.protobuf.ByteString.copyFrom(ownerAddress))
            .setBalance(0)
            .setCreateTime(System.currentTimeMillis())
            .setType(org.tron.protos.Protocol.AccountType.Normal);
        issuerAccount = new org.tron.core.capsule.AccountCapsule(accountBuilder.build());
      }

      // Update asset maps
      if (createV1) {
        // V1: asset map by name
        issuerAccount.addAsset(assetIssued.getName(), assetIssued.getTotalSupply());
        logger.debug("Updated issuer V1 asset map: {}={}", name, assetIssued.getTotalSupply());
      }

      // V2: assetV2 map by token ID
      issuerAccount.addAssetV2(tokenId.getBytes(), assetIssued.getTotalSupply());
      logger.debug("Updated issuer V2 asset map: {}={}", tokenId, assetIssued.getTotalSupply());

      // Store updated account
      accountStore.put(ownerAddress, issuerAccount);

      // Mark account as dirty for resource processor synchronization
      org.tron.core.storage.sync.ResourceSyncContext.recordAccountDirty(ownerAddress);

      logger.info("Successfully applied AssetIssued change: owner={}, name={}, tokenId={}, allowSameTokenName={}",
          addressStr, name, tokenId, allowSameTokenName);

    } catch (Exception e) {
      logger.error("Failed to apply AssetIssued change for owner: {}, error: {}",
          org.tron.common.utils.StringUtil.encode58Check(assetIssued.getOwnerAddress()),
          e.getMessage(), e);
    }
  }

  /**
   * Apply a TRC-10 asset transfer change to update account asset balances.
   * Handles both V1 (asset map by name) and V2 (assetV2 map by token_id) storage.
   */
  private void applyAssetTransferredChange(ExecutionSPI.Trc10AssetTransferred assetTransferred,
                                           ChainBaseManager chainBaseManager,
                                           TransactionContext context) {
    try {
      byte[] ownerAddress = assetTransferred.getOwnerAddress();
      byte[] toAddress = assetTransferred.getToAddress();
      byte[] assetName = assetTransferred.getAssetName();
      String tokenId = assetTransferred.getTokenId();
      long amount = assetTransferred.getAmount();

      // If tokenId is missing (V1 path), derive it from AssetIssueStore using asset name.
      if (tokenId == null || tokenId.isEmpty()) {
        try {
          org.tron.core.store.AssetIssueStore assetIssueStore =
              chainBaseManager.getAssetIssueStore();
          if (assetIssueStore != null && assetName != null) {
            org.tron.core.capsule.AssetIssueCapsule assetIssue =
                assetIssueStore.get(assetName);
            if (assetIssue != null && assetIssue.getId() != null) {
              tokenId = assetIssue.getId();
              logger.debug("Derived TRC-10 tokenId '{}' from asset name for transfer application",
                  tokenId);
            }
          }
        } catch (Exception e) {
          logger.warn("Failed to derive tokenId from AssetIssueStore: {}", e.getMessage());
        }
      }

      String ownerStr = org.tron.common.utils.StringUtil.encode58Check(ownerAddress);
      String toStr = org.tron.common.utils.StringUtil.encode58Check(toAddress);

      logger.info("Applying TRC-10 transfer: owner={}, to={}, amount={}, tokenId={}",
          ownerStr, toStr, amount, tokenId);

      // Get stores
      org.tron.core.store.DynamicPropertiesStore dynamicStore =
          chainBaseManager.getDynamicPropertiesStore();
      org.tron.core.store.AccountStore accountStore =
          chainBaseManager.getAccountStore();

      // Determine V1 vs V2 mode
      long allowSameTokenName = dynamicStore.getAllowSameTokenName();
      boolean useV2 = (allowSameTokenName != 0) &&
          (tokenId != null && !tokenId.isEmpty());

      logger.debug("TRC-10 transfer mode: allowSameTokenName={}, tokenId={}, useV2={}",
          allowSameTokenName, tokenId, useV2);

      // 1. Load owner account
      org.tron.core.capsule.AccountCapsule ownerAccount = accountStore.get(ownerAddress);
      if (ownerAccount == null) {
        logger.error("Owner account not found for TRC-10 transfer: {}", ownerStr);
        return;
      }

      // 2. Validate owner has sufficient TRC-10 balance
      String assetKey;
      if (useV2) {
        assetKey = tokenId;
      } else {
        assetKey = new String(assetName, java.nio.charset.StandardCharsets.UTF_8);
      }

      Long ownerBalanceObj = ownerAccount.getAsset(dynamicStore, assetKey);
      long ownerBalance = (ownerBalanceObj != null) ? ownerBalanceObj : 0;

      if (ownerBalance < amount) {
        logger.error("Insufficient TRC-10 balance: owner has {}, needs {}", ownerBalance, amount);
        return;
      }

      // 3. Load or create recipient account
      org.tron.core.capsule.AccountCapsule recipientAccount = accountStore.get(toAddress);
      if (recipientAccount == null) {
        // Create recipient account (matches TransferAssetActuator behavior)
        logger.debug("Creating recipient account for TRC-10 transfer: {}", toStr);
        org.tron.protos.Protocol.Account.Builder accountBuilder =
            org.tron.protos.Protocol.Account.newBuilder()
            .setAddress(com.google.protobuf.ByteString.copyFrom(toAddress))
            .setBalance(0)
            .setCreateTime(System.currentTimeMillis())
            .setType(org.tron.protos.Protocol.AccountType.Normal);
        recipientAccount = new org.tron.core.capsule.AccountCapsule(accountBuilder.build());
      }

      // 4. Update balances using the same methods as TransferAssetActuator
      // Both V1 and V2 should use reduceAssetAmountV2/addAssetAmountV2 which handle
      // both modes internally and record TRC-10 balance changes for CSV journaling.
      byte[] assetKeyBytes;
      org.tron.core.store.AssetIssueStore assetIssueStore;
      if (useV2) {
        // V2 mode: use token ID as key
        assetKeyBytes = tokenId.getBytes();
        assetIssueStore = chainBaseManager.getAssetIssueV2Store();
      } else {
        // V1 mode: use asset name as key (same as TransferAssetActuator)
        assetKeyBytes = assetName;
        assetIssueStore = chainBaseManager.getAssetIssueStore();
      }

      if (!ownerAccount.reduceAssetAmountV2(assetKeyBytes, amount, dynamicStore, assetIssueStore)) {
        logger.error("reduceAssetAmountV2 failed for owner {}", ownerStr);
        return;
      }
      recipientAccount.addAssetAmountV2(assetKeyBytes, amount, dynamicStore, assetIssueStore);

      logger.debug("Updated asset balances (useV2={}): owner {} -= {}, recipient {} += {}",
          useV2, ownerStr, amount, toStr, amount);

      // 5. Persist accounts
      accountStore.put(ownerAddress, ownerAccount);
      accountStore.put(toAddress, recipientAccount);

      // Mark accounts as dirty for resource processor synchronization
      org.tron.core.storage.sync.ResourceSyncContext.recordAccountDirty(ownerAddress);
      org.tron.core.storage.sync.ResourceSyncContext.recordAccountDirty(toAddress);

      logger.info("Successfully applied TRC-10 transfer: owner={}, to={}, amount={}, tokenId={}, useV2={}",
          ownerStr, toStr, amount, tokenId, useV2);

    } catch (Exception e) {
      logger.error("Failed to apply TRC-10 transfer change: owner={}, to={}, error: {}",
          org.tron.common.utils.StringUtil.encode58Check(assetTransferred.getOwnerAddress()),
          org.tron.common.utils.StringUtil.encode58Check(assetTransferred.getToAddress()),
          e.getMessage(), e);
    }
  }

  /**
   * Apply a single state change to the local database.
   */
  private void applyStateChange(ExecutionSPI.StateChange stateChange,
                               ChainBaseManager chainBaseManager,
                               TransactionContext context) {
    try {
      byte[] address = stateChange.getAddress();
      byte[] key = stateChange.getKey();
      byte[] oldValue = stateChange.getOldValue();
      byte[] newValue = stateChange.getNewValue();
      
      // Log state change details for debugging
      logger.debug("Applying state change - address: {}, key length: {}, oldValue length: {}, newValue length: {}",
          org.tron.common.utils.ByteArray.toHexString(address),
          key != null ? key.length : 0,
          oldValue != null ? oldValue.length : 0,
          newValue != null ? newValue.length : 0);
      
      // For account balance changes (key is typically empty or null)
      // This indicates an account-level change (balance, nonce, code, etc.)
      if (key == null || key.length == 0) {
        // This is an account balance/state update
        logger.debug("Processing account state change for address: {}", 
                    org.tron.common.utils.StringUtil.encode58Check(address));
        updateAccountState(address, newValue, chainBaseManager, context);
      } else {
        // This is a storage update (contract storage slot)
        logger.debug("Processing storage change for address: {}, key: {}", 
                    org.tron.common.utils.StringUtil.encode58Check(address),
                    org.tron.common.utils.ByteArray.toHexString(key));
        updateAccountStorage(address, key, newValue, chainBaseManager, context);
      }
      
    } catch (Exception e) {
      logger.error("Failed to apply individual state change for address: {}, error: {}", 
          org.tron.common.utils.ByteArray.toHexString(stateChange.getAddress()), 
          e.getMessage(), e);
    }
  }

  /**
   * Update account state (balance, nonce, etc.) in the local database.
   */
  private void updateAccountState(byte[] address, byte[] newValue, 
                                 ChainBaseManager chainBaseManager,
                                 TransactionContext context) {
    try {
      // Log the address format for debugging
      logger.info("Updating account state for address (length: {}): {}, newValue length: {}", 
          address.length, org.tron.common.utils.ByteArray.toHexString(address), 
          newValue != null ? newValue.length : 0);
      
      String addressStr = org.tron.common.utils.StringUtil.encode58Check(address);
      
      // Check for account deletion
      if (newValue == null || newValue.length == 0) {
        // Handle account deletion
        AccountCapsule existingAccount = chainBaseManager.getAccountStore().get(address);
        if (existingAccount != null) {
          // Delete the account from the store
          chainBaseManager.getAccountStore().delete(address);
          logger.info("Deleted account: {} due to remote execution state sync", addressStr);
        } else {
          logger.debug("Account deletion requested for non-existent account: {}", addressStr);
        }
        return;
      }
      
      // Deserialize the AccountInfo from the serialized format first
      AccountInfo accountInfo = deserializeAccountInfo(newValue);
      if (accountInfo == null) {
        logger.error("Failed to deserialize AccountInfo for address: {} from {} bytes", addressStr, newValue.length);
        // Don't proceed if we can't deserialize the account info
        return;
      }
      
      // Get or create account
      AccountCapsule accountCapsule = chainBaseManager.getAccountStore().get(address);
      boolean isNewAccount = (accountCapsule == null);
      
      if (isNewAccount) {
        // Create new account if it doesn't exist with the balance from AccountInfo
        Account.Builder accountBuilder = Account.newBuilder()
            .setAddress(com.google.protobuf.ByteString.copyFrom(address))
            .setBalance(accountInfo.balance) // Use balance from AccountInfo
            .setCreateTime(System.currentTimeMillis())
            .setType(org.tron.protos.Protocol.AccountType.Normal); // Set account type
        accountCapsule = new AccountCapsule(accountBuilder.build());
        logger.info("Created new account: {} with balance: {} for remote execution state sync", 
                   addressStr, accountInfo.balance);
      } else {
        // Update existing account
        long oldBalance = accountCapsule.getBalance();        
        // Update balance
        accountCapsule.setBalance(accountInfo.balance);
        
        logger.info("Updated existing account {}: balance {} -> {}", 
                   addressStr, oldBalance, accountInfo.balance);
      }
      
      // Note: TRON doesn't have explicit nonce like Ethereum, so we'll just track it for logging
      // Note: Getting/Setting contract code in TRON requires different mechanisms than just accessing AccountCapsule
      // This would typically involve ContractStore and other TRON-specific storage
      if (accountInfo.code != null && accountInfo.code.length > 0) {
        logger.debug("Account {} has contract code: {} bytes, codeHash: {}",
                    addressStr, accountInfo.code.length,
                    org.tron.common.utils.ByteArray.toHexString(accountInfo.codeHash));
        // TODO: Handle contract code storage if needed
      }

      // Apply resource usage fields from AEXT tail if present
      if (accountInfo.hasResourceUsage()) {
        logger.debug("Applying AEXT resource usage fields for account: {}", addressStr);

        // Set usage fields
        if (accountInfo.netUsage != null) {
          accountCapsule.setNetUsage(accountInfo.netUsage);
        }
        if (accountInfo.freeNetUsage != null) {
          accountCapsule.setFreeNetUsage(accountInfo.freeNetUsage);
        }
        if (accountInfo.energyUsage != null) {
          accountCapsule.setEnergyUsage(accountInfo.energyUsage);
        }

        // Set timing fields
        if (accountInfo.latestConsumeTime != null) {
          accountCapsule.setLatestConsumeTime(accountInfo.latestConsumeTime);
        }
        if (accountInfo.latestConsumeFreeTime != null) {
          accountCapsule.setLatestConsumeFreeTime(accountInfo.latestConsumeFreeTime);
        }
        if (accountInfo.latestConsumeTimeForEnergy != null) {
          accountCapsule.setLatestConsumeTimeForEnergy(accountInfo.latestConsumeTimeForEnergy);
        }

        // Set window size and optimization flags
        if (accountInfo.netWindowSize != null) {
          accountCapsule.setNewWindowSize(BANDWIDTH, accountInfo.netWindowSize);
        }
        if (accountInfo.energyWindowSize != null) {
          accountCapsule.setNewWindowSize(ENERGY, accountInfo.energyWindowSize);
        }
        if (accountInfo.netWindowOptimized != null) {
          accountCapsule.setWindowOptimized(BANDWIDTH, accountInfo.netWindowOptimized);
        }
        if (accountInfo.energyWindowOptimized != null) {
          accountCapsule.setWindowOptimized(ENERGY, accountInfo.energyWindowOptimized);
        }

        logger.debug("Applied resource usage to account {}: netUsage={}, freeNetUsage={}, energyUsage={}, times=[{},{},{}], windows=[{},{}], optimized=[{},{}]",
                     addressStr, accountInfo.netUsage, accountInfo.freeNetUsage, accountInfo.energyUsage,
                     accountInfo.latestConsumeTime, accountInfo.latestConsumeFreeTime, accountInfo.latestConsumeTimeForEnergy,
                     accountInfo.netWindowSize, accountInfo.energyWindowSize,
                     accountInfo.netWindowOptimized, accountInfo.energyWindowOptimized);
      }

      // Store the updated account
      chainBaseManager.getAccountStore().put(address, accountCapsule);
      
      if (isNewAccount) {
        logger.info("Successfully created and stored new account: {} with balance: {}", 
                   addressStr, accountInfo.balance);
      } else {
        logger.info("Successfully updated existing account: {} with new balance: {}", 
                   addressStr, accountInfo.balance);
      }
      
    } catch (Exception e) {
      logger.error("Failed to update account state for address: {}, error: {}", 
          org.tron.common.utils.StringUtil.encode58Check(address), e.getMessage(), e);
    }
  }

  /**
   * Update account storage in the local database.
   */
  private void updateAccountStorage(byte[] address, byte[] key, byte[] newValue,
                                   ChainBaseManager chainBaseManager,
                                   TransactionContext context) {
    try {
      // Account storage updates would go here
      // This is more complex and depends on how Account storage is managed
      logger.debug("Account storage update for address: {}, key: {}", 
          address, key);
      // TODO: Implement account storage synchronization if needed
      
    } catch (Exception e) {
      logger.warn("Failed to update account storage for address: {}, key: {}, error: {}", 
          address, key, e.getMessage());
    }
  }

  /**
   * Convert byte array to long (big-endian).
   */
  private long bytesToLong(byte[] bytes) {
    if (bytes == null || bytes.length < 8) {
      return 0;
    }
    long result = 0;
    for (int i = 0; i < 8; i++) {
      result = (result << 8) | (bytes[i] & 0xFF);
    }
    return result;
  }

  /**
   * Convert 32-byte balance array to long (big-endian).
   */
  private long bytesToLongFromBalance(byte[] bytes) {
    if (bytes == null || bytes.length < 32) {
      return 0;
    }
    long result = 0;
    // Take the last 8 bytes from the 32-byte balance
    for (int i = 24; i < 32; i++) {
      result = (result << 8) | (bytes[i] & 0xFF);
    }
    return result;
  }

  /**
   * Simple AccountInfo class to hold deserialized account information.
   * Extended to support AEXT (Account EXTension) resource usage fields.
   */
  private static class AccountInfo {
    public final long balance;
    public final long nonce;
    public final byte[] codeHash;
    public final byte[] code;

    // AEXT v1 resource usage fields (optional, null if not present)
    public final Long netUsage;
    public final Long freeNetUsage;
    public final Long energyUsage;
    public final Long latestConsumeTime;
    public final Long latestConsumeFreeTime;
    public final Long latestConsumeTimeForEnergy;
    public final Long netWindowSize;
    public final Long energyWindowSize;
    public final Boolean netWindowOptimized;
    public final Boolean energyWindowOptimized;

    public AccountInfo(long balance, long nonce, byte[] codeHash, byte[] code) {
      this(balance, nonce, codeHash, code, null, null, null, null, null, null, null, null, null, null);
    }

    public AccountInfo(long balance, long nonce, byte[] codeHash, byte[] code,
                       Long netUsage, Long freeNetUsage, Long energyUsage,
                       Long latestConsumeTime, Long latestConsumeFreeTime, Long latestConsumeTimeForEnergy,
                       Long netWindowSize, Long energyWindowSize,
                       Boolean netWindowOptimized, Boolean energyWindowOptimized) {
      this.balance = balance;
      this.nonce = nonce;
      this.codeHash = codeHash != null ? codeHash : new byte[0];
      this.code = code != null ? code : new byte[0];
      this.netUsage = netUsage;
      this.freeNetUsage = freeNetUsage;
      this.energyUsage = energyUsage;
      this.latestConsumeTime = latestConsumeTime;
      this.latestConsumeFreeTime = latestConsumeFreeTime;
      this.latestConsumeTimeForEnergy = latestConsumeTimeForEnergy;
      this.netWindowSize = netWindowSize;
      this.energyWindowSize = energyWindowSize;
      this.netWindowOptimized = netWindowOptimized;
      this.energyWindowOptimized = energyWindowOptimized;
    }

    public boolean hasResourceUsage() {
      return netUsage != null;
    }
  }

  /**
   * Deserialize AccountInfo from byte array.
   * Format: [balance(32)] + [nonce(8)] + [code_hash(32)] + [code_length(4)] + [code(variable)]
   */
  private AccountInfo deserializeAccountInfo(byte[] data) {
    // Handle empty data for account deletion cases
    if (data == null || data.length == 0) {
      return null;
    }
    
    // Handle minimal accounts (balance only) - at least 32 bytes for balance
    if (data.length < 32) {
      logger.warn("AccountInfo data too short: {} bytes. Expected at least 32 bytes for balance.", data.length);
      return null;
    }
    
    try {
      int offset = 0;
      
      // Extract balance (32 bytes, big-endian)
      byte[] balanceBytes = new byte[32];
      System.arraycopy(data, offset, balanceBytes, 0, 32);
      long balance = bytesToLongFromBalance(balanceBytes);
      offset += 32;
      
      // Default values for optional fields
      long nonce = 0;
      byte[] codeHash = new byte[32]; // Default to zero hash
      byte[] code = new byte[0]; // Default to empty code
      
      // Extract nonce if present (8 bytes, big-endian)
      if (data.length >= offset + 8) {
        for (int i = 0; i < 8; i++) {
          nonce = (nonce << 8) | (data[offset + i] & 0xFF);
        }
        offset += 8;
        
        // Extract code hash if present (32 bytes)
        if (data.length >= offset + 32) {
          System.arraycopy(data, offset, codeHash, 0, 32);
          offset += 32;
          
          // Extract code length and code if present (4 bytes for length, then variable)
          if (data.length >= offset + 4) {
            int codeLength = 0;
            for (int i = 0; i < 4; i++) {
              codeLength = (codeLength << 8) | (data[offset + i] & 0xFF);
            }
            offset += 4;

            // Extract code (variable length)
            if (codeLength > 0 && data.length >= offset + codeLength) {
              code = new byte[codeLength];
              System.arraycopy(data, offset, code, 0, codeLength);
              offset += codeLength;
            }
          }
        }
      }

      // Try to parse optional AEXT (Account EXTension) tail for resource usage
      Long netUsage = null, freeNetUsage = null, energyUsage = null;
      Long latestConsumeTime = null, latestConsumeFreeTime = null, latestConsumeTimeForEnergy = null;
      Long netWindowSize = null, energyWindowSize = null;
      Boolean netWindowOptimized = null, energyWindowOptimized = null;

      if (offset + 4 <= data.length) {
        // Check for AEXT magic: 0x41 0x45 0x58 0x54 ("AEXT")
        if (data[offset] == 0x41 && data[offset + 1] == 0x45 &&
            data[offset + 2] == 0x58 && data[offset + 3] == 0x54) {
          offset += 4;

          try {
            // Read version (u16 big-endian)
            if (offset + 2 > data.length) {
              logger.warn("AEXT tail truncated at version field");
            } else {
              int version = ((data[offset] & 0xFF) << 8) | (data[offset + 1] & 0xFF);
              offset += 2;

              if (version != 1) {
                logger.warn("AEXT version {} not supported, skipping tail", version);
              } else {
                // Read length (u16 big-endian)
                if (offset + 2 > data.length) {
                  logger.warn("AEXT tail truncated at length field");
                } else {
                  int payloadLength = ((data[offset] & 0xFF) << 8) | (data[offset + 1] & 0xFF);
                  offset += 2;

                  if (offset + payloadLength > data.length) {
                    logger.warn("AEXT payload length {} exceeds remaining data {}", payloadLength, data.length - offset);
                  } else {
                    // Parse AEXT v1 payload (all big-endian i64 except booleans)
                    int payloadOffset = offset;

                    // Helper to read i64 big-endian
                    java.util.function.Function<Integer, Long> readI64 = (off) -> {
                      long val = 0;
                      for (int i = 0; i < 8; i++) {
                        val = (val << 8) | (data[off + i] & 0xFF);
                      }
                      return val;
                    };

                    if (payloadLength >= 68) { // Minimum payload size: 8*8 (64) + 1 + 1 + 2 = 68
                      netUsage = readI64.apply(payloadOffset);
                      freeNetUsage = readI64.apply(payloadOffset + 8);
                      energyUsage = readI64.apply(payloadOffset + 16);
                      latestConsumeTime = readI64.apply(payloadOffset + 24);
                      latestConsumeFreeTime = readI64.apply(payloadOffset + 32);
                      latestConsumeTimeForEnergy = readI64.apply(payloadOffset + 40);
                      netWindowSize = readI64.apply(payloadOffset + 48);
                      energyWindowSize = readI64.apply(payloadOffset + 56);
                      netWindowOptimized = data[payloadOffset + 64] != 0;
                      energyWindowOptimized = data[payloadOffset + 65] != 0;
                      // Reserved/padding bytes at payloadOffset + 66, 67 are ignored

                      logger.debug("Parsed AEXT v1: netUsage={}, freeNetUsage={}, energyUsage={}, times=[{},{},{}], windows=[{},{}], optimized=[{},{}]",
                                   netUsage, freeNetUsage, energyUsage,
                                   latestConsumeTime, latestConsumeFreeTime, latestConsumeTimeForEnergy,
                                   netWindowSize, energyWindowSize,
                                   netWindowOptimized, energyWindowOptimized);
                    } else {
                      logger.warn("AEXT payload length {} too short for v1 (expected >= 68)", payloadLength);
                    }
                  }
                }
              }
            }
          } catch (Exception e) {
            logger.warn("Failed to parse AEXT tail: {}", e.getMessage());
            // Continue without resource usage fields
          }
        }
      }

      logger.debug("Deserialized AccountInfo - balance: {}, nonce: {}, codeHash length: {}, code length: {}, hasResourceUsage: {}",
                   balance, nonce, codeHash.length, code.length, (netUsage != null));

      return new AccountInfo(balance, nonce, codeHash, code,
                             netUsage, freeNetUsage, energyUsage,
                             latestConsumeTime, latestConsumeFreeTime, latestConsumeTimeForEnergy,
                             netWindowSize, energyWindowSize,
                             netWindowOptimized, energyWindowOptimized);

    } catch (Exception e) {
      logger.warn("Failed to deserialize AccountInfo from {} bytes: {}", data.length, e.getMessage());
      return null;
    }
  }

  /**
   * Capture pre-state snapshot for CSV reporting in remote execution mode.
   * This captures TRC-10 balances, votes, and global totals BEFORE applying changes,
   * allowing the builder to compute absolute old/new values for domain triplets.
   *
   * <p>Gated by: -Dremote.exec.prestate.snapshot.enabled=true (default true).
   */
  private void capturePreStateSnapshot(ExecutionProgramResult result, TransactionContext context) {
    // Check JVM gate: default true
    boolean captureEnabled = Boolean.parseBoolean(
        System.getProperty("remote.exec.prestate.snapshot.enabled", "true"));

    if (!captureEnabled) {
      logger.debug("Pre-state snapshot capture disabled by JVM property for transaction: {}",
          context.getTrxCap().getTransactionId());
      return;
    }

    try {
      ChainBaseManager chainBaseManager = context.getStoreFactory().getChainBaseManager();
      org.tron.core.store.AccountStore accountStore = chainBaseManager.getAccountStore();
      org.tron.core.store.DynamicPropertiesStore dynamicStore =
          chainBaseManager.getDynamicPropertiesStore();

      // Initialize snapshot for this transaction
      PreStateSnapshotRegistry.initializeForCurrentTransaction();

      // 1. Capture TRC-10 balances for addresses involved in transfers
      if (result.getTrc10Changes() != null) {
        for (ExecutionSPI.Trc10Change trc10Change : result.getTrc10Changes()) {
          if (trc10Change.hasAssetTransferred()) {
            ExecutionSPI.Trc10AssetTransferred transfer = trc10Change.getAssetTransferred();
            String tokenId = transfer.getTokenId();

            // If tokenId is missing (V1 path), derive it from AssetIssueStore using asset name.
            if (tokenId == null || tokenId.isEmpty()) {
              try {
                org.tron.core.store.AssetIssueStore assetIssueStore =
                    chainBaseManager.getAssetIssueStore();
                if (assetIssueStore != null && transfer.getAssetName() != null) {
                  org.tron.core.capsule.AssetIssueCapsule assetIssue =
                      assetIssueStore.get(transfer.getAssetName());
                  if (assetIssue != null && assetIssue.getId() != null) {
                    tokenId = assetIssue.getId();
                    logger.debug("Derived TRC-10 tokenId '{}' from asset name for prestate snapshot",
                        tokenId);
                  }
                }
              } catch (Exception e) {
                logger.warn("Failed to derive tokenId from AssetIssueStore: {}", e.getMessage());
              }
            }

            // Capture owner's pre-state balance
            byte[] ownerAddress = transfer.getOwnerAddress();
            AccountCapsule ownerAccount = accountStore.get(ownerAddress);
            if (ownerAccount != null && tokenId != null) {
              Map<String, Long> assetV2Map = ownerAccount.getAssetMapV2();
              Long ownerBalance = assetV2Map.get(tokenId);
              PreStateSnapshotRegistry.captureTrc10Balance(
                  ownerAddress, tokenId, ownerBalance != null ? ownerBalance : 0L);
            }

            // Capture recipient's pre-state balance
            byte[] toAddress = transfer.getToAddress();
            AccountCapsule recipientAccount = accountStore.get(toAddress);
            if (recipientAccount != null && tokenId != null) {
              Map<String, Long> recipientAssetV2Map = recipientAccount.getAssetMapV2();
              Long recipientBalance = recipientAssetV2Map.get(tokenId);
              PreStateSnapshotRegistry.captureTrc10Balance(
                  toAddress, tokenId, recipientBalance != null ? recipientBalance : 0L);
            } else if (tokenId != null) {
              // Recipient account doesn't exist yet, balance is 0
              PreStateSnapshotRegistry.captureTrc10Balance(toAddress, tokenId, 0L);
            }
          }
        }
      }

      // 2. Capture votes for voters involved in vote changes
      if (result.getVoteChanges() != null) {
        for (ExecutionSPI.VoteChange voteChange : result.getVoteChanges()) {
          byte[] voterAddress = voteChange.getOwnerAddress();
          AccountCapsule voterAccount = accountStore.get(voterAddress);
          if (voterAccount != null) {
            // Capture all existing votes for this voter
            java.util.List<Vote> existingVotes = voterAccount.getVotesList();
            PreStateSnapshotRegistry.captureVotes(voterAddress, existingVotes);
          }
        }
      }

      // 3. Capture global resource totals (for freeze/global resource changes)
      boolean hasFreezeChanges = result.getFreezeChanges() != null
          && !result.getFreezeChanges().isEmpty();
      boolean hasGlobalChanges = result.getGlobalResourceChanges() != null
          && !result.getGlobalResourceChanges().isEmpty();

      if (hasFreezeChanges || hasGlobalChanges) {
        long totalNetWeight = dynamicStore.getTotalNetWeight();
        long totalNetLimit = dynamicStore.getTotalNetLimit();
        long totalEnergyWeight = dynamicStore.getTotalEnergyWeight();
        long totalEnergyLimit = dynamicStore.getTotalEnergyCurrentLimit();
        long totalTronPowerWeight = dynamicStore.getTotalTronPowerWeight();

        PreStateSnapshotRegistry.captureGlobalTotals(
            totalNetWeight, totalNetLimit, totalEnergyWeight, totalEnergyLimit, totalTronPowerWeight);
      }

      // 3b. Capture freeze snapshots (owner/resource old amount + expire) before applying changes
      if (result.getFreezeChanges() != null) {
        for (ExecutionSPI.FreezeLedgerChange freezeChange : result.getFreezeChanges()) {
          byte[] ownerAddress = freezeChange.getOwnerAddress();
          if (ownerAddress == null) {
            continue;
          }

          AccountCapsule ownerAccount = accountStore.get(ownerAddress);
          long oldAmount = 0L;
          long oldExpireTimeMs = 0L;

          if (ownerAccount != null) {
            switch (freezeChange.getResource()) {
              case BANDWIDTH:
                if (freezeChange.isV2Model()) {
                  // V2 has no expiration
                  oldAmount = ownerAccount.getFrozenV2BalanceForBandwidth();
                  oldExpireTimeMs = 0L;
                } else {
                  oldAmount = ownerAccount.getFrozenBalance();
                  java.util.List<org.tron.protos.Protocol.Account.Frozen> frozenList = ownerAccount.getFrozenList();
                  oldExpireTimeMs = (frozenList != null && !frozenList.isEmpty())
                      ? frozenList.get(0).getExpireTime() : 0L;
                }
                break;
              case ENERGY:
                if (freezeChange.isV2Model()) {
                  oldAmount = ownerAccount.getFrozenV2BalanceForEnergy();
                  oldExpireTimeMs = 0L;
                } else {
                  oldAmount = ownerAccount.getEnergyFrozenBalance();
                  oldExpireTimeMs = ownerAccount.getAccountResource()
                      .getFrozenBalanceForEnergy().getExpireTime();
                }
                break;
              case TRON_POWER:
                if (freezeChange.isV2Model()) {
                  oldAmount = ownerAccount.getTronPowerFrozenV2Balance();
                  oldExpireTimeMs = 0L;
                } else {
                  oldAmount = ownerAccount.getTronPowerFrozenBalance();
                  oldExpireTimeMs = ownerAccount.getInstance().getTronPower().getExpireTime();
                }
                break;
              default:
                // Unknown resource type: leave zeros
                break;
            }
          }

          // Note: recipient is not provided by FreezeLedgerChange; use self-freeze (null recipient)
          PreStateSnapshotRegistry.captureFreeze(
              ownerAddress, freezeChange.getResource().name(), null, oldAmount, oldExpireTimeMs);
        }
      }

      // 4. Capture per-account frozen totals for limit computation
      // Build set of affected addresses from state changes (empty key = account changes)
      // and freeze changes owners
      Set<String> affectedAddresses = new HashSet<>();
      List<StateChange> stateChanges = result.getStateChanges();
      if (stateChanges != null) {
        for (StateChange sc : stateChanges) {
          // Empty key means account state change
          if (sc.getKey() == null || sc.getKey().length == 0) {
            if (sc.getAddress() != null) {
              affectedAddresses.add(org.tron.common.utils.ByteArray.toHexString(sc.getAddress()).toLowerCase());
            }
          }
        }
      }
      // Also include freeze change owners
      if (result.getFreezeChanges() != null) {
        for (ExecutionSPI.FreezeLedgerChange freezeChange : result.getFreezeChanges()) {
          if (freezeChange.getOwnerAddress() != null) {
            affectedAddresses.add(
                org.tron.common.utils.ByteArray.toHexString(freezeChange.getOwnerAddress()).toLowerCase());
          }
        }
      }

      // Capture frozen totals for each affected address
      for (String addressHex : affectedAddresses) {
        byte[] addressBytes = org.tron.common.utils.ByteArray.fromHexString(addressHex);
        AccountCapsule account = accountStore.get(addressBytes);
        if (account != null) {
          long frozenForBandwidth = account.getAllFrozenBalanceForBandwidth();
          long frozenForEnergy = account.getAllFrozenBalanceForEnergy();
          PreStateSnapshotRegistry.captureAccountFrozenTotals(
              addressBytes, frozenForBandwidth, frozenForEnergy);
        }
      }

      logger.debug("Captured pre-state snapshot for transaction: {} - {}",
          context.getTrxCap().getTransactionId(),
          PreStateSnapshotRegistry.getCurrentSnapshotMetrics());

    } catch (Exception e) {
      logger.warn("Failed to capture pre-state snapshot for transaction: {}, error: {}",
          context.getTrxCap().getTransactionId(), e.getMessage());
      // Don't fail the transaction - snapshot is for reporting only
    }
  }
}

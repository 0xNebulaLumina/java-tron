package org.tron.core.execution.reporting;

import java.util.ArrayList;
import java.util.List;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.db.TransactionTrace;
import org.tron.core.execution.spi.ExecutionProgramResult;
import org.tron.core.execution.spi.ExecutionSPI;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;
import org.tron.core.execution.spi.ExecutionSPI.Trc10LedgerChange;
import org.tron.core.store.AccountStore;
import org.tron.core.store.DynamicPropertiesStore;
import org.tron.core.ChainBaseManager;

/**
 * Synthesizer for creating account-level StateChange entries from TRC-10 ledger changes.
 *
 * <p>This class bridges the gap between remote execution (which emits TRC-10 effects as
 * separate trc10Changes) and embedded execution (which records all account balance changes
 * as StateChange entries). By synthesizing StateChange entries from trc10Changes, we achieve
 * CSV parity between remote and embedded execution modes.
 *
 * <p>Strategy:
 * - Read post-apply account snapshots from the AccountStore
 * - Reverse deltas (fees, amounts) to reconstruct pre-apply "old" snapshots
 * - Serialize both old and new using AccountInfoCodec
 * - Build StateChange entries with address, empty key, old bytes, new bytes
 *
 * <p>Operations supported:
 * - AssetIssue: Owner fee debit + blackhole credit (or burn)
 * - ParticipateAssetIssue: Owner pays TRX, issuer receives TRX
 *
 * <p>Configuration:
 * - exec.csv.include.trc10 (default true): Enable TRC-10 synthesis
 * - exec.csv.ledger.strict (default false): Fail closed if synthesis cannot complete
 */
public class LedgerCsvSynthesizer {

  private static final Logger logger = LoggerFactory.getLogger(LedgerCsvSynthesizer.class);

  /**
   * System property to enable TRC-10 ledger synthesis in CSV.
   * Default: true
   */
  public static final String PROPERTY_INCLUDE_TRC10 = "exec.csv.include.trc10";

  /**
   * System property for strict mode.
   * If true, synthesis failure for any change causes entire synthesis to abort.
   * If false, synthesis continues with partial results.
   * Default: false
   */
  public static final String PROPERTY_STRICT_MODE = "exec.csv.ledger.strict";

  /**
   * Check if TRC-10 synthesis is enabled.
   */
  public static boolean isEnabled() {
    return Boolean.parseBoolean(System.getProperty(PROPERTY_INCLUDE_TRC10, "true"));
  }

  /**
   * Check if strict mode is enabled.
   */
  public static boolean isStrictMode() {
    return Boolean.parseBoolean(System.getProperty(PROPERTY_STRICT_MODE, "false"));
  }

  /**
   * Synthesize account-level StateChange entries from TRC-10 ledger changes.
   *
   * @param execResult Execution result containing TRC-10 changes
   * @param trace Transaction trace for accessing stores
   * @return List of synthesized StateChange entries
   */
  public static List<StateChange> synthesize(ExecutionProgramResult execResult, TransactionTrace trace) {
    List<StateChange> result = new ArrayList<>();

    if (!isEnabled()) {
      logger.debug("TRC-10 synthesis disabled by property");
      return result;
    }

    if (execResult == null || trace == null) {
      logger.debug("Cannot synthesize: null inputs");
      return result;
    }

    List<Trc10LedgerChange> trc10Changes = execResult.getTrc10Changes();
    if (trc10Changes == null || trc10Changes.isEmpty()) {
      logger.debug("No TRC-10 changes to synthesize");
      return result;
    }

    try {
      // Resolve stores
      ChainBaseManager chainBaseManager = trace.getTransactionContext()
          .getStoreFactory()
          .getChainBaseManager();
      DynamicPropertiesStore dynamicStore = chainBaseManager.getDynamicPropertiesStore();
      AccountStore accountStore = chainBaseManager.getAccountStore();

      boolean strictMode = isStrictMode();
      int synthesizedCount = 0;

      // Process each TRC-10 change
      for (Trc10LedgerChange trc10Change : trc10Changes) {
        try {
          List<StateChange> changeList = synthesizeForOperation(trc10Change, dynamicStore, accountStore, strictMode);
          result.addAll(changeList);
          synthesizedCount += changeList.size();
        } catch (Exception e) {
          logger.warn("Failed to synthesize TRC-10 op={}, error: {}", trc10Change.getOp(), e.getMessage());
          if (strictMode) {
            logger.error("Strict mode enabled, aborting TRC-10 synthesis");
            return new ArrayList<>(); // Return empty list in strict mode
          }
          // Continue with other changes in non-strict mode
        }
      }

      logger.info("CSV ledger synthesis: added {} TRC-10 state changes for tx {} (ops={})",
          synthesizedCount, trace.getTransactionContext().getTrxCap().getTransactionId(), trc10Changes.size());

    } catch (Exception e) {
      logger.error("Failed to synthesize TRC-10 changes: {}", e.getMessage(), e);
      if (isStrictMode()) {
        return new ArrayList<>(); // Return empty list in strict mode
      }
    }

    return result;
  }

  /**
   * Synthesize StateChange entries for a single TRC-10 operation.
   */
  private static List<StateChange> synthesizeForOperation(
      Trc10LedgerChange trc10Change,
      DynamicPropertiesStore dynamicStore,
      AccountStore accountStore,
      boolean strictMode) {

    List<StateChange> result = new ArrayList<>();

    switch (trc10Change.getOp()) {
      case ISSUE:
        result.addAll(synthesizeAssetIssue(trc10Change, dynamicStore, accountStore, strictMode));
        break;

      case PARTICIPATE:
        result.addAll(synthesizeParticipate(trc10Change, dynamicStore, accountStore, strictMode));
        break;

      case TRANSFER:
        logger.debug("TRC-10 TRANSFER synthesis not yet implemented (Phase 2)");
        break;

      default:
        logger.warn("Unknown TRC-10 operation type: {}", trc10Change.getOp());
    }

    return result;
  }

  /**
   * Synthesize StateChange entries for AssetIssue operation.
   * Creates entries for:
   * - Owner: balance debit (fee)
   * - Blackhole: balance credit (fee) - only if not burning
   */
  private static List<StateChange> synthesizeAssetIssue(
      Trc10LedgerChange trc10Change,
      DynamicPropertiesStore dynamicStore,
      AccountStore accountStore,
      boolean strictMode) {

    List<StateChange> result = new ArrayList<>();

    try {
      byte[] ownerAddress = trc10Change.getOwnerAddress();

      // Determine fee: use feeSun from Rust if present, otherwise use dynamic store
      long fee = trc10Change.getFeeSun() != null
          ? trc10Change.getFeeSun()
          : dynamicStore.getAssetIssueFee();

      // Determine if blackhole or burn
      boolean useBlackhole = !dynamicStore.supportBlackHoleOptimization();

      logger.debug("Synthesizing ISSUE: owner={}, fee={}, useBlackhole={}",
          org.tron.common.utils.ByteArray.toHexString(ownerAddress), fee, useBlackhole);

      // Synthesize owner change
      StateChange ownerChange = synthesizeAccountBalanceChange(
          ownerAddress, fee, true /* debit */, accountStore);
      if (ownerChange != null) {
        result.add(ownerChange);
        logger.debug("Added owner change: address={}, balance delta=-{}",
            org.tron.common.utils.ByteArray.toHexString(ownerAddress), fee);
      } else if (strictMode) {
        throw new RuntimeException("Failed to synthesize owner change for ISSUE");
      }

      // Synthesize blackhole change (if not burning)
      if (useBlackhole) {
        AccountCapsule blackholeAccount = accountStore.getBlackhole();
        if (blackholeAccount != null) {
          byte[] blackholeAddress = blackholeAccount.getAddress().toByteArray();
          StateChange blackholeChange = synthesizeAccountBalanceChange(
              blackholeAddress, fee, false /* credit */, accountStore);
          if (blackholeChange != null) {
            result.add(blackholeChange);
            logger.debug("Added blackhole change: address={}, balance delta=+{}",
                org.tron.common.utils.ByteArray.toHexString(blackholeAddress), fee);
          } else if (strictMode) {
            throw new RuntimeException("Failed to synthesize blackhole change for ISSUE");
          }
        } else {
          logger.warn("Blackhole account not found");
          if (strictMode) {
            throw new RuntimeException("Blackhole account not found for ISSUE");
          }
        }
      } else {
        logger.debug("Burning enabled, no blackhole change synthesized");
      }

    } catch (Exception e) {
      logger.warn("Failed to synthesize ISSUE: {}", e.getMessage());
      if (strictMode) {
        throw e;
      }
    }

    return result;
  }

  /**
   * Synthesize StateChange entries for ParticipateAssetIssue operation.
   * Creates entries for:
   * - Owner: balance debit (trxAmount)
   * - Issuer: balance credit (trxAmount)
   */
  private static List<StateChange> synthesizeParticipate(
      Trc10LedgerChange trc10Change,
      DynamicPropertiesStore dynamicStore,
      AccountStore accountStore,
      boolean strictMode) {

    List<StateChange> result = new ArrayList<>();

    try {
      byte[] ownerAddress = trc10Change.getOwnerAddress();
      byte[] issuerAddress = trc10Change.getToAddress();
      long trxAmount = trc10Change.getAmount();

      logger.debug("Synthesizing PARTICIPATE: owner={}, issuer={}, trxAmount={}",
          org.tron.common.utils.ByteArray.toHexString(ownerAddress),
          org.tron.common.utils.ByteArray.toHexString(issuerAddress),
          trxAmount);

      // Synthesize owner change (pays TRX)
      StateChange ownerChange = synthesizeAccountBalanceChange(
          ownerAddress, trxAmount, true /* debit */, accountStore);
      if (ownerChange != null) {
        result.add(ownerChange);
        logger.debug("Added owner change: address={}, balance delta=-{}",
            org.tron.common.utils.ByteArray.toHexString(ownerAddress), trxAmount);
      } else if (strictMode) {
        throw new RuntimeException("Failed to synthesize owner change for PARTICIPATE");
      }

      // Synthesize issuer change (receives TRX)
      StateChange issuerChange = synthesizeAccountBalanceChange(
          issuerAddress, trxAmount, false /* credit */, accountStore);
      if (issuerChange != null) {
        result.add(issuerChange);
        logger.debug("Added issuer change: address={}, balance delta=+{}",
            org.tron.common.utils.ByteArray.toHexString(issuerAddress), trxAmount);
      } else if (strictMode) {
        throw new RuntimeException("Failed to synthesize issuer change for PARTICIPATE");
      }

    } catch (Exception e) {
      logger.warn("Failed to synthesize PARTICIPATE: {}", e.getMessage());
      if (strictMode) {
        throw e;
      }
    }

    return result;
  }

  /**
   * Synthesize a StateChange for an account balance change.
   * Reads the post-apply account from store, clones it, and adjusts balance to get pre-apply state.
   *
   * @param address Account address
   * @param amount Balance delta amount
   * @param isDebit true for debit (subtract from new to get old), false for credit (add to new to get old)
   * @param accountStore Account store to read post-apply state
   * @return StateChange entry, or null if synthesis failed
   */
  private static StateChange synthesizeAccountBalanceChange(
      byte[] address,
      long amount,
      boolean isDebit,
      AccountStore accountStore) {

    try {
      // Read post-apply account (new state)
      AccountCapsule newAccount = accountStore.get(address);
      if (newAccount == null) {
        logger.warn("Account not found for synthesis: address={}",
            org.tron.common.utils.ByteArray.toHexString(address));
        return null;
      }

      long newBalance = newAccount.getBalance();

      // Calculate old balance by reversing the delta
      long oldBalance;
      if (isDebit) {
        // Debit: old = new + amount (account had more before)
        oldBalance = newBalance + amount;
      } else {
        // Credit: old = new - amount (account had less before)
        oldBalance = newBalance - amount;
      }

      // Clone account and set old balance
      AccountCapsule oldAccount = new AccountCapsule(newAccount.getData().clone());
      oldAccount.setBalance(oldBalance);

      // Serialize both using AccountInfoCodec
      byte[] oldBytes = AccountInfoCodec.serialize(oldAccount);
      byte[] newBytes = AccountInfoCodec.serialize(newAccount);

      // Build StateChange with empty key (indicates account-level change)
      StateChange change = new StateChange(
          address.clone(),
          new byte[0], // Empty key for account change
          oldBytes,
          newBytes
      );

      logger.debug("Synthesized balance change: address={}, old={}, new={}, delta={}{}",
          org.tron.common.utils.ByteArray.toHexString(address),
          oldBalance, newBalance,
          isDebit ? "-" : "+", amount);

      return change;

    } catch (Exception e) {
      logger.warn("Failed to synthesize account balance change: address={}, error={}",
          org.tron.common.utils.ByteArray.toHexString(address), e.getMessage());
      return null;
    }
  }
}

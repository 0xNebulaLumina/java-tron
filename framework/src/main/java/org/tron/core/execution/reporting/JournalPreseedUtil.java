package org.tron.core.execution.reporting;

import com.google.protobuf.Any;
import com.google.protobuf.InvalidProtocolBufferException;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.db.StateChangeRecorderContext;
import org.tron.core.db.TransactionTrace;
import org.tron.core.store.AccountStore;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.Protocol.Transaction.Contract.ContractType;
import org.tron.protos.contract.BalanceContract.TransferContract;

/**
 * Utility for preseeding the state change journal with post-resource account snapshots.
 *
 * <p>This ensures that embedded CSV state changes reflect account balances after pre-execution
 * resource consumption (bandwidth, create-account fees), aligning with remote execution CSV format.
 *
 * <p>The journal's merge logic keeps the first oldAccount and updates newAccount on subsequent
 * writes, so preseeding establishes the correct baseline for oldValue in CSV output.
 *
 * <p>Design context: {@code planning/post-fee.planning.md}
 */
public class JournalPreseedUtil {

  private static final Logger logger = LoggerFactory.getLogger(JournalPreseedUtil.class);

  // Feature flag: default enabled, can be disabled via -Dexec.csv.preseedAfterResource=false
  private static final String PRESEED_ENABLED_PROP = "exec.csv.preseedAfterResource";
  private static final String PRESEED_DEBUG_PROP = "exec.csv.preseed.debug";

  /**
   * Attempt to preseed the journal with post-resource account snapshots.
   *
   * <p>Call this immediately after:
   * <ul>
   *   <li>ResourceSyncContext.flushPreExec() (resource mutations applied)</li>
   *   <li>StateChangeJournalRegistry.initializeForCurrentTransaction()</li>
   *   <li>StateChangeRecorderContext.setRecorder(...)</li>
   * </ul>
   *
   * <p>And before:
   * <ul>
   *   <li>trace.exec() (VM execution)</li>
   * </ul>
   *
   * @param trace Transaction trace with access to transaction context and stores
   */
  public static void tryPreseedAfterResource(TransactionTrace trace) {
    // Gate 1: Journal must be enabled
    if (!StateChangeJournal.isEnabled()) {
      return;
    }

    // Gate 2: Preseed flag must be enabled (default: true)
    String preseedEnabledStr = System.getProperty(PRESEED_ENABLED_PROP, "true");
    if (!"true".equalsIgnoreCase(preseedEnabledStr)) {
      if (logger.isDebugEnabled()) {
        logger.debug("CSV preseed disabled via {}", PRESEED_ENABLED_PROP);
      }
      return;
    }

    // Gate 3: Recorder must be active
    if (!StateChangeRecorderContext.isEnabled()) {
      return;
    }

    try {
      TransactionCapsule trxCap = trace.getTransactionContext().getTrxCap();
      Transaction.Contract contract = trxCap.getInstance().getRawData().getContract(0);
      ContractType contractType = contract.getType();

      // Phase 1 scope: TransferContract only
      if (contractType != ContractType.TransferContract) {
        return;
      }

      preseedTransferContract(trace, contract);

    } catch (Exception e) {
      // Don't fail transaction on preseed errors, just log
      logger.warn("CSV preseed failed, continuing without preseed", e);
    }
  }

  /**
   * Preseed addresses involved in a TransferContract.
   *
   * <p>Preseeds:
   * <ul>
   *   <li>Owner (sender): always, if exists in store</li>
   *   <li>Recipient: only if already exists (skip creation case to avoid old==new noise)</li>
   * </ul>
   */
  private static void preseedTransferContract(TransactionTrace trace,
                                              Transaction.Contract contract)
      throws InvalidProtocolBufferException {

    Any parameter = contract.getParameter();
    TransferContract transferContract = parameter.unpack(TransferContract.class);

    byte[] ownerAddress = transferContract.getOwnerAddress().toByteArray();
    byte[] toAddress = transferContract.getToAddress().toByteArray();

    AccountStore accountStore = trace.getTransactionContext()
        .getStoreFactory()
        .getChainBaseManager()
        .getAccountStore();

    int seededCount = 0;
    boolean debugEnabled = "true".equalsIgnoreCase(System.getProperty(PRESEED_DEBUG_PROP, "false"));

    // Preseed owner (sender) - should always exist
    AccountCapsule ownerAccount = accountStore.getUnchecked(ownerAddress);
    if (ownerAccount != null) {
      // Clone to avoid mutation issues (journal should capture immutable snapshot)
      AccountCapsule ownerSnapshot = new AccountCapsule(ownerAccount.getData());
      StateChangeRecorderContext.recordAccountChange(ownerAddress, ownerSnapshot, ownerSnapshot);
      seededCount++;

      if (debugEnabled) {
        logger.debug("CSV preseed owner: address={} balance={}",
                     org.tron.common.utils.ByteArray.toHexString(ownerAddress),
                     ownerSnapshot.getBalance());
      }
    } else {
      logger.warn("CSV preseed: owner account not found for address={}",
                  org.tron.common.utils.ByteArray.toHexString(ownerAddress));
    }

    // Preseed recipient only if it already exists (skip creation case)
    AccountCapsule toAccount = accountStore.getUnchecked(toAddress);
    if (toAccount != null) {
      AccountCapsule toSnapshot = new AccountCapsule(toAccount.getData());
      StateChangeRecorderContext.recordAccountChange(toAddress, toSnapshot, toSnapshot);
      seededCount++;

      if (debugEnabled) {
        logger.debug("CSV preseed recipient: address={} balance={}",
                     org.tron.common.utils.ByteArray.toHexString(toAddress),
                     toSnapshot.getBalance());
      }
    } else {
      // Recipient doesn't exist yet - will be created during execution
      // Don't preseed to avoid old==new emission
      if (debugEnabled) {
        logger.debug("CSV preseed: recipient does not exist yet, skipping preseed for to={}",
                     org.tron.common.utils.ByteArray.toHexString(toAddress));
      }
    }

    if (logger.isInfoEnabled() && seededCount > 0) {
      logger.info("CSV preseed: owner={} to={} seeded={}",
                  org.tron.common.utils.ByteArray.toHexString(ownerAddress),
                  org.tron.common.utils.ByteArray.toHexString(toAddress),
                  seededCount);
    }
  }
}

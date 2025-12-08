package org.tron.core.execution.reporting;

import java.util.List;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.core.execution.reporting.DomainCanonicalizer.FreezeDelta;
import org.tron.core.execution.reporting.DomainCanonicalizer.GlobalResourceDelta;
import org.tron.core.execution.reporting.DomainCanonicalizer.Trc10BalanceDelta;
import org.tron.core.execution.reporting.DomainCanonicalizer.Trc10IssuanceDelta;
import org.tron.core.execution.reporting.DomainCanonicalizer.VoteDelta;

/**
 * Thread-local registry for DomainChangeJournal instances.
 *
 * <p>This allows various components throughout the transaction execution
 * lifecycle (actuators, stores) to access the same journal instance for
 * recording domain-specific changes.
 *
 * <p>Usage pattern:
 * <pre>
 *   // At transaction start
 *   DomainChangeJournalRegistry.initializeForCurrentTransaction();
 *
 *   // During execution
 *   DomainChangeJournalRegistry.recordTrc10BalanceChange(...);
 *   DomainChangeJournalRegistry.recordVoteChange(...);
 *
 *   // At transaction end
 *   List&lt;Trc10BalanceDelta&gt; trc10Changes = DomainChangeJournalRegistry.getTrc10BalanceChanges();
 *   DomainChangeJournalRegistry.clearForCurrentTransaction();
 * </pre>
 */
public class DomainChangeJournalRegistry {

  private static final Logger logger = LoggerFactory.getLogger(DomainChangeJournalRegistry.class);

  private static final ThreadLocal<DomainChangeJournal> journalThreadLocal = new ThreadLocal<>();

  /**
   * Initialize a new journal for the current transaction thread.
   * This should be called once at the beginning of each transaction.
   */
  public static void initializeForCurrentTransaction() {
    if (!DomainChangeJournal.isEnabled()) {
      return; // Don't create journal if disabled
    }

    DomainChangeJournal existing = journalThreadLocal.get();
    if (existing != null) {
      logger.warn("DomainChangeJournal already exists for current thread, clearing previous journal");
      existing.clear();
    }

    DomainChangeJournal journal = new DomainChangeJournal();
    journalThreadLocal.set(journal);

    if (logger.isDebugEnabled()) {
      logger.debug("Initialized DomainChangeJournal for transaction thread {}",
                   Thread.currentThread().getId());
    }
  }

  // ================================
  // TRC-10 Balance Recording
  // ================================

  /**
   * Record a TRC-10 balance change in the current transaction's journal.
   */
  public static void recordTrc10BalanceChange(byte[] ownerAddress, String tokenId,
                                              long oldBalance, long newBalance) {
    DomainChangeJournal journal = journalThreadLocal.get();
    if (journal != null) {
      journal.recordTrc10BalanceChange(ownerAddress, tokenId, oldBalance, newBalance);
    }
  }

  /**
   * Get TRC-10 balance changes from current journal.
   */
  public static List<Trc10BalanceDelta> getTrc10BalanceChanges() {
    DomainChangeJournal journal = journalThreadLocal.get();
    if (journal != null) {
      return journal.getTrc10BalanceChanges();
    }
    return java.util.Collections.emptyList();
  }

  // ================================
  // TRC-10 Issuance Recording
  // ================================

  /**
   * Record a TRC-10 issuance change in the current transaction's journal.
   */
  public static void recordTrc10IssuanceChange(String tokenId, String field,
                                               String oldValue, String newValue, String op) {
    DomainChangeJournal journal = journalThreadLocal.get();
    if (journal != null) {
      journal.recordTrc10IssuanceChange(tokenId, field, oldValue, newValue, op);
    }
  }

  /**
   * Get TRC-10 issuance changes from current journal.
   */
  public static List<Trc10IssuanceDelta> getTrc10IssuanceChanges() {
    DomainChangeJournal journal = journalThreadLocal.get();
    if (journal != null) {
      return journal.getTrc10IssuanceChanges();
    }
    return java.util.Collections.emptyList();
  }

  // ================================
  // Vote Recording
  // ================================

  /**
   * Record a vote change in the current transaction's journal.
   */
  public static void recordVoteChange(byte[] voterAddress, byte[] witnessAddress,
                                      long oldVotes, long newVotes) {
    DomainChangeJournal journal = journalThreadLocal.get();
    if (journal != null) {
      journal.recordVoteChange(voterAddress, witnessAddress, oldVotes, newVotes);
    }
  }

  /**
   * Get vote changes from current journal.
   */
  public static List<VoteDelta> getVoteChanges() {
    DomainChangeJournal journal = journalThreadLocal.get();
    if (journal != null) {
      return journal.getVoteChanges();
    }
    return java.util.Collections.emptyList();
  }

  // ================================
  // Freeze Recording
  // ================================

  /**
   * Record a freeze change in the current transaction's journal.
   */
  public static void recordFreezeChange(byte[] ownerAddress, String resourceType,
                                        byte[] recipientAddress,
                                        long oldAmount, long newAmount,
                                        long oldExpireTime, long newExpireTime,
                                        String op) {
    DomainChangeJournal journal = journalThreadLocal.get();
    if (journal != null) {
      journal.recordFreezeChange(ownerAddress, resourceType, recipientAddress,
                                 oldAmount, newAmount, oldExpireTime, newExpireTime, op);
    }
  }

  /**
   * Get freeze changes from current journal.
   */
  public static List<FreezeDelta> getFreezeChanges() {
    DomainChangeJournal journal = journalThreadLocal.get();
    if (journal != null) {
      return journal.getFreezeChanges();
    }
    return java.util.Collections.emptyList();
  }

  // ================================
  // Global Resource Recording
  // ================================

  /**
   * Record a global resource change in the current transaction's journal.
   */
  public static void recordGlobalResourceChange(String field, long oldValue, long newValue) {
    DomainChangeJournal journal = journalThreadLocal.get();
    if (journal != null) {
      journal.recordGlobalResourceChange(field, oldValue, newValue);
    }
  }

  /**
   * Get global resource changes from current journal.
   */
  public static List<GlobalResourceDelta> getGlobalResourceChanges() {
    DomainChangeJournal journal = journalThreadLocal.get();
    if (journal != null) {
      return journal.getGlobalResourceChanges();
    }
    return java.util.Collections.emptyList();
  }

  // ================================
  // Journal Management
  // ================================

  /**
   * Get current journal metrics (for monitoring).
   */
  public static String getCurrentJournalMetrics() {
    DomainChangeJournal journal = journalThreadLocal.get();
    if (journal == null) {
      return "No domain journal active";
    }
    return String.format("DomainJournal: %d TRC-10 balance, %d TRC-10 issuance, " +
                         "%d votes, %d freezes, %d global resources",
                         journal.getTrc10BalanceChangeCount(),
                         journal.getTrc10IssuanceChangeCount(),
                         journal.getVoteChangeCount(),
                         journal.getFreezeChangeCount(),
                         journal.getGlobalResourceChangeCount());
  }

  /**
   * Clear journal for current thread.
   */
  public static void clearForCurrentTransaction() {
    DomainChangeJournal journal = journalThreadLocal.get();
    if (journal != null) {
      journal.clear();
      journalThreadLocal.remove();
      if (logger.isDebugEnabled()) {
        logger.debug("Cleared DomainChangeJournal for transaction thread {}",
                     Thread.currentThread().getId());
      }
    }
  }

  /**
   * Check if a journal is active for the current thread.
   */
  public static boolean hasActiveJournal() {
    return journalThreadLocal.get() != null;
  }

  /**
   * Get the current journal (for advanced use cases).
   */
  public static DomainChangeJournal getCurrentJournal() {
    return journalThreadLocal.get();
  }
}

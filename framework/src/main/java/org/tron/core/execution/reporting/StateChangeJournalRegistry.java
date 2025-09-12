package org.tron.core.execution.reporting;

import java.util.List;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;

/**
 * Thread-local registry for StateChangeJournal instances.
 * 
 * <p>This allows various components throughout the transaction execution
 * lifecycle (VM, storage, repository operations) to access the same
 * journal instance for recording state changes.
 * 
 * <p>Usage pattern:
 * <pre>
 *   // At transaction start
 *   StateChangeJournalRegistry.initializeForCurrentTransaction();
 *   
 *   // During execution
 *   StateChangeJournalRegistry.recordStorageChange(...);
 *   StateChangeJournalRegistry.recordAccountChange(...);
 *   
 *   // At transaction end
 *   List&lt;StateChange&gt; changes = StateChangeJournalRegistry.finalizeForCurrentTransaction();
 * </pre>
 */
public class StateChangeJournalRegistry {
  
  private static final Logger logger = LoggerFactory.getLogger(StateChangeJournalRegistry.class);
  
  private static final ThreadLocal<StateChangeJournal> journalThreadLocal = new ThreadLocal<>();
  
  /**
   * Initialize a new journal for the current transaction thread.
   * This should be called once at the beginning of each transaction.
   */
  public static void initializeForCurrentTransaction() {
    if (!StateChangeJournal.isEnabled()) {
      return; // Don't create journal if disabled
    }
    
    StateChangeJournal existing = journalThreadLocal.get();
    if (existing != null) {
      logger.warn("StateChangeJournal already exists for current thread, clearing previous journal");
      existing.clear();
    }
    
    StateChangeJournal journal = new StateChangeJournal();
    journalThreadLocal.set(journal);
    
    if (logger.isDebugEnabled()) {
      logger.debug("Initialized StateChangeJournal for transaction thread {}", 
                   Thread.currentThread().getId());
    }
  }
  
  /**
   * Record a storage change in the current transaction's journal.
   */
  public static void recordStorageChange(byte[] contractAddress, byte[] storageKey, 
                                         byte[] oldValue, byte[] newValue) {
    StateChangeJournal journal = journalThreadLocal.get();
    if (journal != null) {
      journal.recordStorageChange(contractAddress, storageKey, oldValue, newValue);
    }
  }
  
  /**
   * Record an account change in the current transaction's journal.
   */
  public static void recordAccountChange(byte[] address, 
                                         org.tron.core.capsule.AccountCapsule oldAccount, 
                                         org.tron.core.capsule.AccountCapsule newAccount) {
    StateChangeJournal journal = journalThreadLocal.get();
    if (journal != null) {
      journal.recordAccountChange(address, oldAccount, newAccount);
    }
  }
  
  /**
   * Get the current state changes without finalizing the journal.
   * This does NOT clear the journal from thread-local storage.
   * 
   * @return List of current state changes, or empty list if no journal or disabled
   */
  public static List<StateChange> getCurrentTransactionStateChanges() {
    StateChangeJournal journal = journalThreadLocal.get();
    if (journal == null) {
      return java.util.Collections.emptyList();
    }
    
    try {
      List<StateChange> changes = journal.getCurrentChanges();
      
      if (logger.isDebugEnabled()) {
        logger.debug("Retrieved current StateChanges for transaction thread {}: {} changes", 
                     Thread.currentThread().getId(), changes.size());
      }
      
      return changes;
    } catch (Exception e) {
      logger.warn("Failed to retrieve current StateChanges", e);
      return java.util.Collections.emptyList();
    }
  }
  
  /**
   * Finalize the current transaction's journal and return state changes.
   * This clears the journal from the thread-local storage.
   * 
   * @return List of state changes, or empty list if no journal or disabled
   */
  public static List<StateChange> finalizeForCurrentTransaction() {
    StateChangeJournal journal = journalThreadLocal.get();
    if (journal == null) {
      return java.util.Collections.emptyList();
    }
    
    try {
      List<StateChange> changes = journal.finalizeChanges();
      
      if (logger.isDebugEnabled()) {
        logger.debug("Finalized StateChangeJournal for transaction thread {}: {} changes", 
                     Thread.currentThread().getId(), changes.size());
      }
      
      return changes;
    } catch (Exception e) {
      logger.warn("Failed to finalize StateChangeJournal", e);
      return java.util.Collections.emptyList();
    } finally {
      // Always clear the journal from thread-local storage
      journalThreadLocal.remove();
    }
  }
  
  /**
   * Get current journal metrics (for monitoring).
   */
  public static String getCurrentJournalMetrics() {
    StateChangeJournal journal = journalThreadLocal.get();
    if (journal == null) {
      return "No journal active";
    }
    return String.format("Journal: %d storage changes, %d account changes",
                         journal.getStorageChangeCount(),
                         journal.getAccountChangeCount());
  }
  
  /**
   * Clear journal for current thread (emergency cleanup).
   */
  public static void clearForCurrentTransaction() {
    StateChangeJournal journal = journalThreadLocal.get();
    if (journal != null) {
      journal.clear();
      journalThreadLocal.remove();
      logger.debug("Cleared StateChangeJournal for transaction thread {}", 
                   Thread.currentThread().getId());
    }
  }
  
  /**
   * Check if a journal is active for the current thread.
   */
  public static boolean hasActiveJournal() {
    return journalThreadLocal.get() != null;
  }
}
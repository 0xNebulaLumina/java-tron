package org.tron.core.db;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Thread-local context for DomainChangeRecorder instances.
 *
 * <p>This allows actuators and stores to record domain-specific changes
 * without directly depending on the framework CSV logging infrastructure.
 * The framework can provide the actual implementation via setRecorder().
 */
public class DomainChangeRecorderContext {

  private static final Logger logger = LoggerFactory.getLogger(DomainChangeRecorderContext.class);

  private static final ThreadLocal<DomainChangeRecorder> recorderThreadLocal = new ThreadLocal<>();

  /**
   * Set the domain change recorder for the current transaction thread.
   * This should be called by the framework at transaction start.
   */
  public static void setRecorder(DomainChangeRecorder recorder) {
    recorderThreadLocal.set(recorder);

    if (logger.isDebugEnabled()) {
      logger.debug("Set DomainChangeRecorder for thread {}: {}",
                   Thread.currentThread().getId(),
                   recorder != null ? recorder.getClass().getSimpleName() : "null");
    }
  }

  /**
   * Get the current recorder, or DISABLED if none set.
   */
  public static DomainChangeRecorder getRecorder() {
    DomainChangeRecorder recorder = recorderThreadLocal.get();
    return recorder != null ? recorder : DomainChangeRecorder.DISABLED;
  }

  // ================================
  // TRC-10 Balance Recording
  // ================================

  /**
   * Record a TRC-10 balance change via the current recorder.
   */
  public static void recordTrc10BalanceChange(byte[] ownerAddress, String tokenId,
                                              long oldBalance, long newBalance) {
    getRecorder().recordTrc10BalanceChange(ownerAddress, tokenId, oldBalance, newBalance);
  }

  // ================================
  // TRC-10 Issuance Recording
  // ================================

  /**
   * Record a TRC-10 issuance change via the current recorder.
   */
  public static void recordTrc10IssuanceChange(String tokenId, String field,
                                               String oldValue, String newValue, String op) {
    getRecorder().recordTrc10IssuanceChange(tokenId, field, oldValue, newValue, op);
  }

  // ================================
  // Vote Recording
  // ================================

  /**
   * Record a vote change via the current recorder.
   */
  public static void recordVoteChange(byte[] voterAddress, byte[] witnessAddress,
                                      long oldVotes, long newVotes) {
    getRecorder().recordVoteChange(voterAddress, witnessAddress, oldVotes, newVotes);
  }

  // ================================
  // Freeze Recording
  // ================================

  /**
   * Record a freeze change via the current recorder.
   */
  public static void recordFreezeChange(byte[] ownerAddress, String resourceType,
                                        byte[] recipientAddress,
                                        long oldAmount, long newAmount,
                                        long oldExpireTime, long newExpireTime,
                                        String op) {
    getRecorder().recordFreezeChange(ownerAddress, resourceType, recipientAddress,
                                     oldAmount, newAmount, oldExpireTime, newExpireTime, op);
  }

  // ================================
  // Global Resource Recording
  // ================================

  /**
   * Record a global resource change via the current recorder.
   */
  public static void recordGlobalResourceChange(String field, long oldValue, long newValue) {
    getRecorder().recordGlobalResourceChange(field, oldValue, newValue);
  }

  /**
   * Check if recording is enabled for current thread.
   */
  public static boolean isEnabled() {
    return getRecorder().isEnabled();
  }

  /**
   * Clear the recorder for current thread.
   */
  public static void clear() {
    DomainChangeRecorder recorder = recorderThreadLocal.get();
    if (recorder != null) {
      recorderThreadLocal.remove();
      if (logger.isDebugEnabled()) {
        logger.debug("Cleared DomainChangeRecorder for thread {}",
                     Thread.currentThread().getId());
      }
    }
  }
}

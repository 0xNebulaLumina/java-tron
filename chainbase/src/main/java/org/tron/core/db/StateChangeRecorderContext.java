package org.tron.core.db;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Thread-local context for StateChangeRecorder instances.
 * 
 * <p>This allows actuator and other components to record state changes
 * without directly depending on the framework CSV logging infrastructure.
 * The framework can provide the actual implementation via setRecorder().
 */
public class StateChangeRecorderContext {
  
  private static final Logger logger = LoggerFactory.getLogger(StateChangeRecorderContext.class);
  
  private static final ThreadLocal<StateChangeRecorder> recorderThreadLocal = new ThreadLocal<>();
  
  /**
   * Set the state change recorder for the current transaction thread.
   * This should be called by the framework at transaction start.
   */
  public static void setRecorder(StateChangeRecorder recorder) {
    recorderThreadLocal.set(recorder);
    
    if (logger.isDebugEnabled()) {
      logger.debug("Set StateChangeRecorder for thread {}: {}", 
                   Thread.currentThread().getId(), 
                   recorder != null ? recorder.getClass().getSimpleName() : "null");
    }
  }
  
  /**
   * Get the current recorder, or DISABLED if none set.
   */
  public static StateChangeRecorder getRecorder() {
    StateChangeRecorder recorder = recorderThreadLocal.get();
    return recorder != null ? recorder : StateChangeRecorder.DISABLED;
  }
  
  /**
   * Record a storage change via the current recorder.
   */
  public static void recordStorageChange(byte[] contractAddress, byte[] storageKey, 
                                        byte[] oldValue, byte[] newValue) {
    getRecorder().recordStorageChange(contractAddress, storageKey, oldValue, newValue);
  }
  
  /**
   * Record an account change via the current recorder.
   */
  public static void recordAccountChange(byte[] address, 
                                        org.tron.core.capsule.AccountCapsule oldAccount, 
                                        org.tron.core.capsule.AccountCapsule newAccount) {
    getRecorder().recordAccountChange(address, oldAccount, newAccount);
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
    StateChangeRecorder recorder = recorderThreadLocal.get();
    if (recorder != null) {
      recorderThreadLocal.remove();
      if (logger.isDebugEnabled()) {
        logger.debug("Cleared StateChangeRecorder for thread {}", 
                     Thread.currentThread().getId());
      }
    }
  }
}
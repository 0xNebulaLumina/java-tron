package org.tron.core.db;

import org.tron.core.capsule.AccountCapsule;

/**
 * Interface for recording state changes during transaction execution.
 * 
 * <p>This provides a decoupled way for various components (storage, repository)
 * to record state changes without directly depending on the CSV logging infrastructure.
 * 
 * <p>The actual implementation can be provided by the framework module which handles
 * the detailed CSV logging and state change journaling.
 */
public interface StateChangeRecorder {
  
  /**
   * Record a storage slot change (SSTORE operation).
   * 
   * @param contractAddress Contract address
   * @param storageKey Storage slot key  
   * @param oldValue Previous value (null for new slots)
   * @param newValue New value (null for deletion)
   */
  void recordStorageChange(byte[] contractAddress, byte[] storageKey, 
                          byte[] oldValue, byte[] newValue);
  
  /**
   * Record an account-level change (balance, code, creation, deletion).
   * 
   * @param address Account address
   * @param oldAccount Previous account state (null for creation)  
   * @param newAccount New account state (null for deletion)
   */
  void recordAccountChange(byte[] address, AccountCapsule oldAccount, AccountCapsule newAccount);
  
  /**
   * Check if state change recording is enabled.
   */
  boolean isEnabled();
  
  /**
   * No-op implementation for when recording is disabled.
   */
  StateChangeRecorder DISABLED = new StateChangeRecorder() {
    @Override
    public void recordStorageChange(byte[] contractAddress, byte[] storageKey, 
                                   byte[] oldValue, byte[] newValue) {
      // No-op
    }
    
    @Override
    public void recordAccountChange(byte[] address, AccountCapsule oldAccount, 
                                   AccountCapsule newAccount) {
      // No-op
    }
    
    @Override
    public boolean isEnabled() {
      return false;
    }
  };
}
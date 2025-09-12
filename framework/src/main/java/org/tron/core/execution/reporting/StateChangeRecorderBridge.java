package org.tron.core.execution.reporting;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.db.StateChangeRecorder;

/**
 * Bridge implementation that connects the StateChangeRecorder interface
 * from chainbase to the StateChangeJournalRegistry in framework.
 * 
 * <p>This allows actuator components to record state changes via the
 * StateChangeRecorder interface while the actual journaling is handled
 * by the framework's StateChangeJournalRegistry.
 */
public class StateChangeRecorderBridge implements StateChangeRecorder {
  
  private static final Logger logger = LoggerFactory.getLogger(StateChangeRecorderBridge.class);
  
  @Override
  public void recordStorageChange(byte[] contractAddress, byte[] storageKey, 
                                 byte[] oldValue, byte[] newValue) {
    StateChangeJournalRegistry.recordStorageChange(contractAddress, storageKey, oldValue, newValue);
  }
  
  @Override
  public void recordAccountChange(byte[] address, AccountCapsule oldAccount, AccountCapsule newAccount) {
    StateChangeJournalRegistry.recordAccountChange(address, oldAccount, newAccount);
  }
  
  @Override
  public boolean isEnabled() {
    return StateChangeJournalRegistry.hasActiveJournal();
  }
}
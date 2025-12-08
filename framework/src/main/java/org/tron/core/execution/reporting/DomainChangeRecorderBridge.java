package org.tron.core.execution.reporting;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.core.db.DomainChangeRecorder;

/**
 * Bridge implementation that connects the DomainChangeRecorder interface
 * from chainbase to the DomainChangeJournalRegistry in framework.
 *
 * <p>This allows actuator components to record domain changes via the
 * DomainChangeRecorder interface while the actual journaling is handled
 * by the framework's DomainChangeJournalRegistry.
 */
public class DomainChangeRecorderBridge implements DomainChangeRecorder {

  private static final Logger logger = LoggerFactory.getLogger(DomainChangeRecorderBridge.class);

  @Override
  public void recordTrc10BalanceChange(byte[] ownerAddress, String tokenId,
                                       long oldBalance, long newBalance) {
    DomainChangeJournalRegistry.recordTrc10BalanceChange(ownerAddress, tokenId,
                                                         oldBalance, newBalance);
  }

  @Override
  public void recordTrc10IssuanceChange(String tokenId, String field,
                                        String oldValue, String newValue, String op) {
    DomainChangeJournalRegistry.recordTrc10IssuanceChange(tokenId, field,
                                                          oldValue, newValue, op);
  }

  @Override
  public void recordVoteChange(byte[] voterAddress, byte[] witnessAddress,
                               long oldVotes, long newVotes) {
    DomainChangeJournalRegistry.recordVoteChange(voterAddress, witnessAddress,
                                                  oldVotes, newVotes);
  }

  @Override
  public void recordFreezeChange(byte[] ownerAddress, String resourceType,
                                 byte[] recipientAddress,
                                 long oldAmount, long newAmount,
                                 long oldExpireTime, long newExpireTime,
                                 String op) {
    DomainChangeJournalRegistry.recordFreezeChange(ownerAddress, resourceType, recipientAddress,
                                                    oldAmount, newAmount,
                                                    oldExpireTime, newExpireTime, op);
  }

  @Override
  public void recordGlobalResourceChange(String field, long oldValue, long newValue) {
    DomainChangeJournalRegistry.recordGlobalResourceChange(field, oldValue, newValue);
  }

  @Override
  public boolean isEnabled() {
    // Use the global feature flag to decide if recording should be attempted.
    // The actual recording methods are no-ops when no journal is active.
    return DomainChangeJournal.isEnabled();
  }
}

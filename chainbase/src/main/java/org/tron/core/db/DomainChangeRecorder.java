package org.tron.core.db;

/**
 * Interface for recording domain-specific changes during transaction execution.
 *
 * <p>This provides a decoupled way for actuators and stores to record domain changes
 * (TRC-10, votes, freezes, global resources) without directly depending on the
 * CSV logging infrastructure in the framework module.
 *
 * <p>The actual implementation is provided by the framework module via
 * DomainChangeRecorderBridge.
 */
public interface DomainChangeRecorder {

  // ================================
  // TRC-10 Balance Changes
  // ================================

  /**
   * Record a TRC-10 balance change.
   *
   * @param ownerAddress Account address
   * @param tokenId Token ID as string
   * @param oldBalance Previous balance
   * @param newBalance New balance
   */
  void recordTrc10BalanceChange(byte[] ownerAddress, String tokenId,
                                long oldBalance, long newBalance);

  // ================================
  // TRC-10 Issuance Changes
  // ================================

  /**
   * Record a TRC-10 issuance metadata change.
   *
   * @param tokenId Token ID as string
   * @param field Field name (e.g., total_supply, description)
   * @param oldValue Previous value
   * @param newValue New value
   * @param op Operation: create, update, delete
   */
  void recordTrc10IssuanceChange(String tokenId, String field,
                                 String oldValue, String newValue, String op);

  // ================================
  // Vote Changes
  // ================================

  /**
   * Record a vote change.
   *
   * @param voterAddress Voter account address
   * @param witnessAddress Witness address being voted for
   * @param oldVotes Previous vote count
   * @param newVotes New vote count
   */
  void recordVoteChange(byte[] voterAddress, byte[] witnessAddress,
                        long oldVotes, long newVotes);

  // ================================
  // Freeze Changes
  // ================================

  /**
   * Record a freeze/unfreeze change.
   *
   * @param ownerAddress Owner account address
   * @param resourceType Resource type: BANDWIDTH, ENERGY, or TRON_POWER
   * @param recipientAddress Recipient address (null if self-freeze)
   * @param oldAmount Previous frozen amount in SUN
   * @param newAmount New frozen amount in SUN
   * @param oldExpireTime Previous expiration timestamp (ms)
   * @param newExpireTime New expiration timestamp (ms)
   * @param op Operation: freeze, unfreeze, update
   */
  void recordFreezeChange(byte[] ownerAddress, String resourceType,
                          byte[] recipientAddress,
                          long oldAmount, long newAmount,
                          long oldExpireTime, long newExpireTime,
                          String op);

  // ================================
  // Global Resource Changes
  // ================================

  /**
   * Record a global resource change.
   *
   * @param field Field name (e.g., total_energy_limit, total_net_weight)
   * @param oldValue Previous value
   * @param newValue New value
   */
  void recordGlobalResourceChange(String field, long oldValue, long newValue);

  /**
   * Check if domain change recording is enabled.
   */
  boolean isEnabled();

  /**
   * No-op implementation for when recording is disabled.
   */
  DomainChangeRecorder DISABLED = new DomainChangeRecorder() {
    @Override
    public void recordTrc10BalanceChange(byte[] ownerAddress, String tokenId,
                                         long oldBalance, long newBalance) {
      // No-op
    }

    @Override
    public void recordTrc10IssuanceChange(String tokenId, String field,
                                          String oldValue, String newValue, String op) {
      // No-op
    }

    @Override
    public void recordVoteChange(byte[] voterAddress, byte[] witnessAddress,
                                 long oldVotes, long newVotes) {
      // No-op
    }

    @Override
    public void recordFreezeChange(byte[] ownerAddress, String resourceType,
                                   byte[] recipientAddress,
                                   long oldAmount, long newAmount,
                                   long oldExpireTime, long newExpireTime,
                                   String op) {
      // No-op
    }

    @Override
    public void recordGlobalResourceChange(String field, long oldValue, long newValue) {
      // No-op
    }

    @Override
    public boolean isEnabled() {
      return false;
    }
  };
}

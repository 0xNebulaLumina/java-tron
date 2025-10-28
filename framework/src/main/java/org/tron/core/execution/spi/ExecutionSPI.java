package org.tron.core.execution.spi;

import java.util.List;
import java.util.concurrent.CompletableFuture;
import org.tron.core.db.TransactionContext;
import org.tron.core.exception.ContractExeException;
import org.tron.core.exception.ContractValidateException;
import org.tron.core.exception.VMIllegalException;

/**
 * Execution Service Provider Interface (SPI) for abstracting EVM execution operations. This
 * interface supports embedded Java EVM, remote Rust execution, and shadow verification.
 */
public interface ExecutionSPI {

  /**
   * Execute a transaction and modify state.
   *
   * @param context Transaction context containing all necessary information
   * @return CompletableFuture with execution result (extends ProgramResult)
   * @throws ContractValidateException if transaction validation fails
   * @throws ContractExeException if transaction execution fails
   * @throws VMIllegalException if VM encounters illegal operation
   */
  CompletableFuture<ExecutionProgramResult> executeTransaction(TransactionContext context)
      throws ContractValidateException, ContractExeException, VMIllegalException;

  /**
   * Call a contract without modifying state (view call).
   *
   * @param context Transaction context for the call
   * @return CompletableFuture with call result (extends ProgramResult)
   * @throws ContractValidateException if call validation fails
   * @throws VMIllegalException if VM encounters illegal operation
   */
  CompletableFuture<ExecutionProgramResult> callContract(TransactionContext context)
      throws ContractValidateException, VMIllegalException;

  /**
   * Estimate energy required for transaction execution.
   *
   * @param context Transaction context for estimation
   * @return CompletableFuture with energy estimate
   * @throws ContractValidateException if transaction validation fails
   */
  CompletableFuture<Long> estimateEnergy(TransactionContext context)
      throws ContractValidateException;

  /**
   * Get contract code at address.
   *
   * @param address Contract address
   * @param snapshotId Optional snapshot ID for historical queries
   * @return CompletableFuture with contract code
   */
  CompletableFuture<byte[]> getCode(byte[] address, String snapshotId);

  /**
   * Get storage value at address and key.
   *
   * @param address Contract address
   * @param key Storage key
   * @param snapshotId Optional snapshot ID for historical queries
   * @return CompletableFuture with storage value
   */
  CompletableFuture<byte[]> getStorageAt(byte[] address, byte[] key, String snapshotId);

  /**
   * Get account nonce.
   *
   * @param address Account address
   * @param snapshotId Optional snapshot ID for historical queries
   * @return CompletableFuture with account nonce
   */
  CompletableFuture<Long> getNonce(byte[] address, String snapshotId);

  /**
   * Get account balance.
   *
   * @param address Account address
   * @param snapshotId Optional snapshot ID for historical queries
   * @return CompletableFuture with account balance
   */
  CompletableFuture<byte[]> getBalance(byte[] address, String snapshotId);

  /**
   * Create EVM snapshot for state rollback.
   *
   * @return CompletableFuture with snapshot ID
   */
  CompletableFuture<String> createSnapshot();

  /**
   * Revert to EVM snapshot.
   *
   * @param snapshotId Snapshot ID to revert to
   * @return CompletableFuture indicating success
   */
  CompletableFuture<Boolean> revertToSnapshot(String snapshotId);

  /**
   * Get execution health status.
   *
   * @return CompletableFuture with health status
   */
  CompletableFuture<HealthStatus> healthCheck();

  /**
   * Register metrics callback for monitoring.
   *
   * @param callback Metrics callback
   */
  void registerMetricsCallback(MetricsCallback callback);

  /** Execution result containing all execution information. */
  class ExecutionResult {
    private final boolean success;
    private final byte[] returnData;
    private final long energyUsed;
    private final long energyRefunded;
    private final List<StateChange> stateChanges;
    private final List<LogEntry> logs;
    private final String errorMessage;
    private final long bandwidthUsed;
    private final List<FreezeLedgerChange> freezeChanges;
    private final List<GlobalResourceTotalsChange> globalResourceChanges;

    public ExecutionResult(
        boolean success,
        byte[] returnData,
        long energyUsed,
        long energyRefunded,
        List<StateChange> stateChanges,
        List<LogEntry> logs,
        String errorMessage,
        long bandwidthUsed,
        List<FreezeLedgerChange> freezeChanges,
        List<GlobalResourceTotalsChange> globalResourceChanges) {
      this.success = success;
      this.returnData = returnData;
      this.energyUsed = energyUsed;
      this.energyRefunded = energyRefunded;
      this.stateChanges = stateChanges;
      this.logs = logs;
      this.errorMessage = errorMessage;
      this.bandwidthUsed = bandwidthUsed;
      this.freezeChanges = freezeChanges;
      this.globalResourceChanges = globalResourceChanges;
    }

    // Getters
    public boolean isSuccess() {
      return success;
    }

    public byte[] getReturnData() {
      return returnData;
    }

    public long getEnergyUsed() {
      return energyUsed;
    }

    public long getEnergyRefunded() {
      return energyRefunded;
    }

    public List<StateChange> getStateChanges() {
      return stateChanges;
    }

    public List<LogEntry> getLogs() {
      return logs;
    }

    public String getErrorMessage() {
      return errorMessage;
    }

    public long getBandwidthUsed() {
      return bandwidthUsed;
    }

    public List<FreezeLedgerChange> getFreezeChanges() {
      return freezeChanges;
    }

    public List<GlobalResourceTotalsChange> getGlobalResourceChanges() {
      return globalResourceChanges;
    }
  }

  /** State change information. */
  class StateChange {
    private final byte[] address;
    private final byte[] key;
    private final byte[] oldValue;
    private final byte[] newValue;

    public StateChange(byte[] address, byte[] key, byte[] oldValue, byte[] newValue) {
      this.address = address;
      this.key = key;
      this.oldValue = oldValue;
      this.newValue = newValue;
    }

    // Getters
    public byte[] getAddress() {
      return address;
    }

    public byte[] getKey() {
      return key;
    }

    public byte[] getOldValue() {
      return oldValue;
    }

    public byte[] getNewValue() {
      return newValue;
    }
  }

  /** Log entry information. */
  class LogEntry {
    private final byte[] address;
    private final List<byte[]> topics;
    private final byte[] data;

    public LogEntry(byte[] address, List<byte[]> topics, byte[] data) {
      this.address = address;
      this.topics = topics;
      this.data = data;
    }

    // Getters
    public byte[] getAddress() {
      return address;
    }

    public List<byte[]> getTopics() {
      return topics;
    }

    public byte[] getData() {
      return data;
    }
  }

  /** Health status information. */
  class HealthStatus {
    private final boolean healthy;
    private final String message;

    public HealthStatus(boolean healthy, String message) {
      this.healthy = healthy;
      this.message = message;
    }

    // Getters
    public boolean isHealthy() {
      return healthy;
    }

    public String getMessage() {
      return message;
    }
  }

  /**
   * Freeze/resource ledger change (Phase 2: emit_freeze_ledger_changes).
   * Describes a single freeze or unfreeze operation affecting an owner's resource balance.
   */
  class FreezeLedgerChange {
    public enum Resource {
      BANDWIDTH(0),
      ENERGY(1),
      TRON_POWER(2);

      private final int value;

      Resource(int value) {
        this.value = value;
      }

      public int getValue() {
        return value;
      }

      public static Resource fromValue(int value) {
        for (Resource r : Resource.values()) {
          if (r.value == value) {
            return r;
          }
        }
        throw new IllegalArgumentException("Unknown resource value: " + value);
      }
    }

    private final byte[] ownerAddress;
    private final Resource resource;
    private final long amount;
    private final long expirationMs;
    private final boolean v2Model;

    public FreezeLedgerChange(byte[] ownerAddress, Resource resource, long amount,
                               long expirationMs, boolean v2Model) {
      this.ownerAddress = ownerAddress;
      this.resource = resource;
      this.amount = amount;
      this.expirationMs = expirationMs;
      this.v2Model = v2Model;
    }

    // Getters
    public byte[] getOwnerAddress() {
      return ownerAddress;
    }

    public Resource getResource() {
      return resource;
    }

    public long getAmount() {
      return amount;
    }

    public long getExpirationMs() {
      return expirationMs;
    }

    public boolean isV2Model() {
      return v2Model;
    }
  }

  /**
   * Global resource totals change (Phase 2: emit_freeze_ledger_changes).
   * Captures dynamic property updates for total weights and limits.
   */
  class GlobalResourceTotalsChange {
    private final long totalNetWeight;
    private final long totalNetLimit;
    private final long totalEnergyWeight;
    private final long totalEnergyLimit;

    public GlobalResourceTotalsChange(long totalNetWeight, long totalNetLimit,
                                       long totalEnergyWeight, long totalEnergyLimit) {
      this.totalNetWeight = totalNetWeight;
      this.totalNetLimit = totalNetLimit;
      this.totalEnergyWeight = totalEnergyWeight;
      this.totalEnergyLimit = totalEnergyLimit;
    }

    // Getters
    public long getTotalNetWeight() {
      return totalNetWeight;
    }

    public long getTotalNetLimit() {
      return totalNetLimit;
    }

    public long getTotalEnergyWeight() {
      return totalEnergyWeight;
    }

    public long getTotalEnergyLimit() {
      return totalEnergyLimit;
    }
  }

  /** Metrics callback interface. */
  interface MetricsCallback {
    void onMetric(String name, double value);
  }
}

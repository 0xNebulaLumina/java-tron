package org.tron.core.execution.reporting;

import org.tron.core.store.DynamicPropertiesStore;

/**
 * Snapshot wrapper for DynamicPropertiesStore that returns pre-state global totals.
 *
 * <p>This wrapper delegates to the underlying DynamicPropertiesStore for most methods,
 * but overrides the global resource total getters to return captured pre-state values.
 * This allows BandwidthProcessor and EnergyProcessor to compute old (pre-execution)
 * limits using the same logic they use for live calculations.
 *
 * <p>Methods overridden for pre-state computation:
 * <ul>
 *   <li>{@link #getTotalNetWeight()}</li>
 *   <li>{@link #getTotalNetLimit()}</li>
 *   <li>{@link #getTotalEnergyWeight()}</li>
 *   <li>{@link #getTotalEnergyCurrentLimit()}</li>
 * </ul>
 *
 * <p>Methods delegated to live store (stable intra-tx):
 * <ul>
 *   <li>{@link #supportUnfreezeDelay()}</li>
 *   <li>{@link #allowNewReward()}</li>
 * </ul>
 */
public class SnapshotDynamicPropertiesStore {

  private final DynamicPropertiesStore delegate;
  private final long snapshotTotalNetWeight;
  private final long snapshotTotalNetLimit;
  private final long snapshotTotalEnergyWeight;
  private final long snapshotTotalEnergyLimit;

  /**
   * Create a snapshot wrapper with pre-state global totals.
   *
   * @param delegate the live DynamicPropertiesStore to delegate non-overridden calls to
   * @param totalNetWeight pre-state total net weight
   * @param totalNetLimit pre-state total net limit
   * @param totalEnergyWeight pre-state total energy weight
   * @param totalEnergyLimit pre-state total energy current limit
   */
  public SnapshotDynamicPropertiesStore(DynamicPropertiesStore delegate,
                                        long totalNetWeight, long totalNetLimit,
                                        long totalEnergyWeight, long totalEnergyLimit) {
    this.delegate = delegate;
    this.snapshotTotalNetWeight = totalNetWeight;
    this.snapshotTotalNetLimit = totalNetLimit;
    this.snapshotTotalEnergyWeight = totalEnergyWeight;
    this.snapshotTotalEnergyLimit = totalEnergyLimit;
  }

  /**
   * Create from PreStateSnapshotRegistry.GlobalSnapshot.
   */
  public static SnapshotDynamicPropertiesStore fromGlobalSnapshot(
      DynamicPropertiesStore delegate,
      PreStateSnapshotRegistry.GlobalSnapshot globals) {
    return new SnapshotDynamicPropertiesStore(
        delegate,
        globals.getTotalNetWeight(),
        globals.getTotalNetLimit(),
        globals.getTotalEnergyWeight(),
        globals.getTotalEnergyLimit()
    );
  }

  // ================================
  // Overridden methods (return snapshot values)
  // ================================

  public long getTotalNetWeight() {
    return snapshotTotalNetWeight;
  }

  public long getTotalNetLimit() {
    return snapshotTotalNetLimit;
  }

  public long getTotalEnergyWeight() {
    return snapshotTotalEnergyWeight;
  }

  public long getTotalEnergyCurrentLimit() {
    return snapshotTotalEnergyLimit;
  }

  // ================================
  // Delegated methods (flags are stable intra-tx)
  // ================================

  public boolean supportUnfreezeDelay() {
    return delegate.supportUnfreezeDelay();
  }

  public boolean allowNewReward() {
    return delegate.allowNewReward();
  }

  /**
   * Get the underlying delegate store for methods not covered by this wrapper.
   */
  public DynamicPropertiesStore getDelegate() {
    return delegate;
  }
}

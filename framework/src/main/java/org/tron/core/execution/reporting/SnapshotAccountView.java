package org.tron.core.execution.reporting;

/**
 * Snapshot view of account frozen balances for computing pre-state limits.
 *
 * <p>This minimal class provides the frozen balance values needed by
 * BandwidthProcessor.calculateGlobalNetLimit() and EnergyProcessor.calculateGlobalEnergyLimit()
 * to compute pre-execution resource limits.
 *
 * <p>Rather than extending AccountCapsule (which is complex and has many dependencies),
 * this class simply holds the pre-captured frozen totals and exposes them through
 * the same method signatures the processors expect.
 */
public class SnapshotAccountView {

  private final long frozenBalanceForBandwidth;
  private final long frozenBalanceForEnergy;

  /**
   * Create a snapshot account view with pre-state frozen balances.
   *
   * @param frozenBalanceForBandwidth pre-state result of getAllFrozenBalanceForBandwidth()
   * @param frozenBalanceForEnergy pre-state result of getAllFrozenBalanceForEnergy()
   */
  public SnapshotAccountView(long frozenBalanceForBandwidth, long frozenBalanceForEnergy) {
    this.frozenBalanceForBandwidth = frozenBalanceForBandwidth;
    this.frozenBalanceForEnergy = frozenBalanceForEnergy;
  }

  /**
   * Create from PreStateSnapshotRegistry.AccountFrozenTotals.
   */
  public static SnapshotAccountView fromAccountFrozenTotals(
      PreStateSnapshotRegistry.AccountFrozenTotals totals) {
    return new SnapshotAccountView(
        totals.getFrozenForBandwidth(),
        totals.getFrozenForEnergy()
    );
  }

  /**
   * Get all frozen balance for bandwidth (same signature as AccountCapsule).
   * This includes: frozen balance + acquired delegated + frozen V2 + acquired delegated V2.
   */
  public long getAllFrozenBalanceForBandwidth() {
    return frozenBalanceForBandwidth;
  }

  /**
   * Get all frozen balance for energy (same signature as AccountCapsule).
   * This includes: energy frozen + acquired delegated + frozen V2 + acquired delegated V2.
   */
  public long getAllFrozenBalanceForEnergy() {
    return frozenBalanceForEnergy;
  }
}

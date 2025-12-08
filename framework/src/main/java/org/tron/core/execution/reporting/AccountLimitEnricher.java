package org.tron.core.execution.reporting;

import java.util.List;
import java.util.Map;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.utils.ByteArray;
import org.tron.core.ChainBaseManager;
import org.tron.core.capsule.AccountCapsule;
import org.tron.core.db.BandwidthProcessor;
import org.tron.core.db.EnergyProcessor;
import org.tron.core.db.TransactionTrace;
import org.tron.core.store.AccountStore;
import org.tron.core.store.DynamicPropertiesStore;

/**
 * Enriches AccountResourceUsageDelta with accurate net_limit and energy_limit values.
 *
 * <p>This class computes per-account limits using the same logic as
 * BandwidthProcessor.calculateGlobalNetLimit() and EnergyProcessor.calculateGlobalEnergyLimit(),
 * ensuring CSV output matches on-chain behavior.
 *
 * <p>Supports both remote and embedded execution modes:
 * <ul>
 *   <li>Remote: Uses PreStateSnapshotRegistry for pre-state frozen totals and global totals</li>
 *   <li>Embedded: Uses DomainChangeJournalRegistry for pre-state data or falls back to live values</li>
 * </ul>
 *
 * <p>The TRX_PRECISION constant (1_000_000) matches chainbase/core/config/Parameter.ChainConstant.
 */
public class AccountLimitEnricher {

  private static final Logger logger = LoggerFactory.getLogger(AccountLimitEnricher.class);

  /** TRX precision: 1 TRX = 1,000,000 SUN */
  private static final long TRX_PRECISION = 1_000_000L;

  /** Execution mode for determining pre-state data source */
  public enum Mode {
    REMOTE,
    EMBEDDED
  }

  /**
   * Enrich AEXT deltas with accurate net_limit and energy_limit values.
   *
   * @param deltas list of AccountResourceUsageDelta to enrich
   * @param trace transaction trace for accessing stores
   * @param mode execution mode (REMOTE or EMBEDDED)
   */
  public static void enrichLimits(
      List<DomainCanonicalizer.AccountResourceUsageDelta> deltas,
      TransactionTrace trace,
      Mode mode) {

    if (deltas == null || deltas.isEmpty()) {
      return;
    }

    // Gate: check if enrichment is enabled (default true)
    boolean enrichEnabled = Boolean.parseBoolean(
        System.getProperty("exec.csv.limit.enrichment.enabled", "true"));
    if (!enrichEnabled) {
      logger.debug("Account limit enrichment disabled by JVM property");
      return;
    }

    try {
      // Get stores from trace context
      if (trace == null || trace.getTransactionContext() == null
          || trace.getTransactionContext().getStoreFactory() == null) {
        logger.debug("Cannot enrich limits: trace/context/stores not available");
        return;
      }

      ChainBaseManager chainBaseManager = trace.getTransactionContext()
          .getStoreFactory().getChainBaseManager();
      if (chainBaseManager == null) {
        logger.debug("Cannot enrich limits: ChainBaseManager not available");
        return;
      }

      DynamicPropertiesStore dynamicStore = chainBaseManager.getDynamicPropertiesStore();
      AccountStore accountStore = chainBaseManager.getAccountStore();

      // Get live processors for computing new (post-state) limits
      BandwidthProcessor bwProcessor = new BandwidthProcessor(chainBaseManager);
      EnergyProcessor energyProcessor = new EnergyProcessor(dynamicStore, accountStore);

      // Get pre-state data based on mode
      PreStateSnapshotRegistry.GlobalSnapshot globals = null;
      Map<String, PreStateSnapshotRegistry.AccountFrozenTotals> accountFrozenMap = null;

      if (mode == Mode.REMOTE) {
        globals = PreStateSnapshotRegistry.getGlobalTotals();
        accountFrozenMap = PreStateSnapshotRegistry.getAllAccountFrozenTotals();
      }
      // For EMBEDDED mode, we'll compute from DomainChangeJournalRegistry below

      // Enrich each delta
      for (DomainCanonicalizer.AccountResourceUsageDelta delta : deltas) {
        String addressHex = delta.getAddressHex();
        if (addressHex == null || addressHex.isEmpty()) {
          continue;
        }

        byte[] addressBytes = ByteArray.fromHexString(addressHex);
        AccountCapsule account = accountStore.get(addressBytes);

        // Compute new (post-state) limits using live account and live stores
        long newNetLimit = 0;
        long newEnergyLimit = 0;
        if (account != null) {
          newNetLimit = bwProcessor.calculateGlobalNetLimit(account);
          newEnergyLimit = energyProcessor.calculateGlobalEnergyLimit(account);
        }

        // Compute old (pre-state) limits
        long oldNetLimit = 0;
        long oldEnergyLimit = 0;

        if (mode == Mode.REMOTE && globals != null && accountFrozenMap != null) {
          // Remote mode: use snapshot data
          PreStateSnapshotRegistry.AccountFrozenTotals frozenTotals =
              accountFrozenMap.get(addressHex.toLowerCase());

          if (frozenTotals != null) {
            // Compute old limits using snapshot inputs
            oldNetLimit = calculateGlobalNetLimitFromSnapshot(
                frozenTotals.getFrozenForBandwidth(),
                globals.getTotalNetWeight(),
                globals.getTotalNetLimit(),
                dynamicStore.supportUnfreezeDelay(),
                dynamicStore.allowNewReward()
            );
            oldEnergyLimit = calculateGlobalEnergyLimitFromSnapshot(
                frozenTotals.getFrozenForEnergy(),
                globals.getTotalEnergyWeight(),
                globals.getTotalEnergyLimit(),
                dynamicStore.supportUnfreezeDelay(),
                dynamicStore.allowNewReward()
            );
          } else {
            // No frozen snapshot for this address, use new = old (no change)
            oldNetLimit = newNetLimit;
            oldEnergyLimit = newEnergyLimit;
            logger.debug("No frozen snapshot for address {}, using post-state as pre-state", addressHex);
          }
        } else if (mode == Mode.EMBEDDED) {
          // Embedded mode: try to reconstruct from DomainChangeJournalRegistry
          oldNetLimit = computeOldLimitEmbedded(addressHex, "BANDWIDTH", dynamicStore);
          oldEnergyLimit = computeOldLimitEmbedded(addressHex, "ENERGY", dynamicStore);

          // If we couldn't compute old, fall back to new
          if (oldNetLimit < 0) {
            oldNetLimit = newNetLimit;
          }
          if (oldEnergyLimit < 0) {
            oldEnergyLimit = newEnergyLimit;
          }
        } else {
          // Fallback: no pre-state data, use new = old
          oldNetLimit = newNetLimit;
          oldEnergyLimit = newEnergyLimit;
        }

        // Set the enriched limit values
        delta.setOldNetLimit(oldNetLimit);
        delta.setNewNetLimit(newNetLimit);
        delta.setOldEnergyLimit(oldEnergyLimit);
        delta.setNewEnergyLimit(newEnergyLimit);

        if (logger.isTraceEnabled()) {
          logger.trace("Enriched limits for {}: net_limit old={} new={}, energy_limit old={} new={}",
              addressHex, oldNetLimit, newNetLimit, oldEnergyLimit, newEnergyLimit);
        }
      }

      logger.debug("Enriched {} AEXT deltas with limit values", deltas.size());

    } catch (Exception e) {
      logger.warn("Failed to enrich account limits: {}", e.getMessage());
      // Don't fail the transaction - enrichment is for reporting only
    }
  }

  /**
   * Calculate global net limit using snapshot values.
   * Mirrors BandwidthProcessor.calculateGlobalNetLimit() logic.
   */
  private static long calculateGlobalNetLimitFromSnapshot(
      long frozeBalance,
      long totalNetWeight,
      long totalNetLimit,
      boolean supportUnfreezeDelay,
      boolean allowNewReward) {

    if (supportUnfreezeDelay) {
      // V2 calculation
      double netWeight = (double) frozeBalance / TRX_PRECISION;
      if (totalNetWeight == 0) {
        return 0;
      }
      return (long) (netWeight * ((double) totalNetLimit / totalNetWeight));
    }

    // V1 calculation
    if (frozeBalance < TRX_PRECISION) {
      return 0;
    }
    long netWeight = frozeBalance / TRX_PRECISION;
    if (allowNewReward && totalNetWeight <= 0) {
      return 0;
    }
    if (totalNetWeight == 0) {
      return 0;
    }
    return (long) (netWeight * ((double) totalNetLimit / totalNetWeight));
  }

  /**
   * Calculate global energy limit using snapshot values.
   * Mirrors EnergyProcessor.calculateGlobalEnergyLimit() logic.
   */
  private static long calculateGlobalEnergyLimitFromSnapshot(
      long frozeBalance,
      long totalEnergyWeight,
      long totalEnergyLimit,
      boolean supportUnfreezeDelay,
      boolean allowNewReward) {

    if (supportUnfreezeDelay) {
      // V2 calculation
      double energyWeight = (double) frozeBalance / TRX_PRECISION;
      if (totalEnergyWeight == 0) {
        return 0;
      }
      return (long) (energyWeight * ((double) totalEnergyLimit / totalEnergyWeight));
    }

    // V1 calculation
    if (frozeBalance < TRX_PRECISION) {
      return 0;
    }
    long energyWeight = frozeBalance / TRX_PRECISION;
    if (allowNewReward && totalEnergyWeight <= 0) {
      return 0;
    }
    // V1 assumes totalEnergyWeight > 0 when not using allowNewReward check
    if (totalEnergyWeight == 0) {
      return 0;
    }
    return (long) (energyWeight * ((double) totalEnergyLimit / totalEnergyWeight));
  }

  /**
   * Compute old limit for embedded mode using DomainChangeJournalRegistry.
   *
   * @param addressHex account address hex
   * @param resourceType "BANDWIDTH" or "ENERGY"
   * @param dynamicStore for flags
   * @return computed old limit, or -1 if cannot compute (caller should fall back to new)
   */
  private static long computeOldLimitEmbedded(
      String addressHex,
      String resourceType,
      DynamicPropertiesStore dynamicStore) {

    // Try to get freeze changes from journal to reconstruct old frozen sums
    List<DomainCanonicalizer.FreezeDelta> freezeDeltas =
        DomainChangeJournalRegistry.getFreezeChanges();

    // Try to get global resource changes from journal
    List<DomainCanonicalizer.GlobalResourceDelta> globalDeltas =
        DomainChangeJournalRegistry.getGlobalResourceChanges();

    // Sum old amounts for this address and resource type
    long oldFrozenSum = 0;
    boolean foundFreezeEntry = false;

    if (freezeDeltas != null) {
      for (DomainCanonicalizer.FreezeDelta fd : freezeDeltas) {
        if (fd.getOwnerAddressHex() != null
            && fd.getOwnerAddressHex().equalsIgnoreCase(addressHex)
            && resourceType.equals(fd.getResourceType())) {
          // Use old amount from freeze delta
          String oldAmountStr = fd.getOldAmountSun();
          if (oldAmountStr != null && !oldAmountStr.isEmpty()) {
            try {
              oldFrozenSum += Long.parseLong(oldAmountStr);
              foundFreezeEntry = true;
            } catch (NumberFormatException e) {
              // Ignore parse errors
            }
          }
        }
      }
    }

    if (!foundFreezeEntry) {
      // No freeze entries for this address/resource, cannot compute old limit
      return -1;
    }

    // Get old global totals from journal
    long oldTotalWeight = 0;
    long oldTotalLimit = 0;
    boolean foundGlobalWeight = false;
    boolean foundGlobalLimit = false;

    String weightKey = resourceType.equals("BANDWIDTH") ? "total_net_weight" : "total_energy_weight";
    String limitKey = resourceType.equals("BANDWIDTH") ? "total_net_limit" : "total_energy_current_limit";

    if (globalDeltas != null) {
      for (DomainCanonicalizer.GlobalResourceDelta gd : globalDeltas) {
        if (weightKey.equals(gd.getField())) {
          String oldVal = gd.getOldValue();
          if (oldVal != null && !oldVal.isEmpty()) {
            try {
              oldTotalWeight = Long.parseLong(oldVal);
              foundGlobalWeight = true;
            } catch (NumberFormatException e) {
              // Ignore
            }
          }
        }
        if (limitKey.equals(gd.getField())) {
          String oldVal = gd.getOldValue();
          if (oldVal != null && !oldVal.isEmpty()) {
            try {
              oldTotalLimit = Long.parseLong(oldVal);
              foundGlobalLimit = true;
            } catch (NumberFormatException e) {
              // Ignore
            }
          }
        }
      }
    }

    // If we couldn't find global totals in journal, read live values (assume unchanged)
    if (!foundGlobalWeight) {
      oldTotalWeight = resourceType.equals("BANDWIDTH")
          ? dynamicStore.getTotalNetWeight()
          : dynamicStore.getTotalEnergyWeight();
    }
    if (!foundGlobalLimit) {
      oldTotalLimit = resourceType.equals("BANDWIDTH")
          ? dynamicStore.getTotalNetLimit()
          : dynamicStore.getTotalEnergyCurrentLimit();
    }

    // Compute old limit using snapshot formula
    if (resourceType.equals("BANDWIDTH")) {
      return calculateGlobalNetLimitFromSnapshot(
          oldFrozenSum, oldTotalWeight, oldTotalLimit,
          dynamicStore.supportUnfreezeDelay(), dynamicStore.allowNewReward());
    } else {
      return calculateGlobalEnergyLimitFromSnapshot(
          oldFrozenSum, oldTotalWeight, oldTotalLimit,
          dynamicStore.supportUnfreezeDelay(), dynamicStore.allowNewReward());
    }
  }
}

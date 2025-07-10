package org.tron.core.storage.spi;

import java.util.HashMap;
import java.util.Map;

/** Statistics information for a storage database. */
public class StorageStats {
  private long totalKeys;
  private long totalSize;
  private Map<String, String> engineStats = new HashMap<>();
  private long lastModified;

  public StorageStats() {}

  public StorageStats(
      long totalKeys, long totalSize, Map<String, String> engineStats, long lastModified) {
    this.totalKeys = totalKeys;
    this.totalSize = totalSize;
    this.engineStats = engineStats != null ? engineStats : new HashMap<>();
    this.lastModified = lastModified;
  }

  // Getters and setters
  public long getTotalKeys() {
    return totalKeys;
  }

  public void setTotalKeys(long totalKeys) {
    this.totalKeys = totalKeys;
  }

  public long getTotalSize() {
    return totalSize;
  }

  public void setTotalSize(long totalSize) {
    this.totalSize = totalSize;
  }

  public Map<String, String> getEngineStats() {
    return engineStats;
  }

  public void setEngineStats(Map<String, String> engineStats) {
    this.engineStats = engineStats;
  }

  public void addEngineStat(String key, String value) {
    this.engineStats.put(key, value);
  }

  public long getLastModified() {
    return lastModified;
  }

  public void setLastModified(long lastModified) {
    this.lastModified = lastModified;
  }

  @Override
  public String toString() {
    return "StorageStats{"
        + "totalKeys="
        + totalKeys
        + ", totalSize="
        + totalSize
        + ", engineStats="
        + engineStats
        + ", lastModified="
        + lastModified
        + '}';
  }
}

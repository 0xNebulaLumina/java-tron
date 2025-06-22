package org.tron.core.storage.spi;

import java.util.HashMap;
import java.util.Map;

/**
 * Configuration class for storage engines.
 */
public class StorageConfig {
    private String engine = "ROCKSDB"; // "ROCKSDB" or "LEVELDB"
    private Map<String, Object> engineOptions = new HashMap<>();
    private boolean enableStatistics = false;
    private int maxOpenFiles = 1000;
    private long blockCacheSize = 8 * 1024 * 1024; // 8MB default

    public StorageConfig() {}

    public StorageConfig(String engine) {
        this.engine = engine;
    }

    // Getters and setters
    public String getEngine() {
        return engine;
    }

    public void setEngine(String engine) {
        this.engine = engine;
    }

    public Map<String, Object> getEngineOptions() {
        return engineOptions;
    }

    public void setEngineOptions(Map<String, Object> engineOptions) {
        this.engineOptions = engineOptions;
    }

    public void addEngineOption(String key, Object value) {
        this.engineOptions.put(key, value);
    }

    public boolean isEnableStatistics() {
        return enableStatistics;
    }

    public void setEnableStatistics(boolean enableStatistics) {
        this.enableStatistics = enableStatistics;
    }

    public int getMaxOpenFiles() {
        return maxOpenFiles;
    }

    public void setMaxOpenFiles(int maxOpenFiles) {
        this.maxOpenFiles = maxOpenFiles;
    }

    public long getBlockCacheSize() {
        return blockCacheSize;
    }

    public void setBlockCacheSize(long blockCacheSize) {
        this.blockCacheSize = blockCacheSize;
    }

    @Override
    public String toString() {
        return "StorageConfig{" +
                "engine='" + engine + '\'' +
                ", engineOptions=" + engineOptions +
                ", enableStatistics=" + enableStatistics +
                ", maxOpenFiles=" + maxOpenFiles +
                ", blockCacheSize=" + blockCacheSize +
                '}';
    }
} 
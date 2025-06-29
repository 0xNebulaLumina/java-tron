package org.tron.core.storage.spi;

/**
 * Enumeration of available storage backend modes.
 * 
 * EMBEDDED - Uses embedded RocksDB via RocksDbDataSourceImpl
 * REMOTE - Uses remote Rust storage service via gRPC
 */
public enum StorageMode {
    /**
     * Embedded RocksDB storage - runs in the same JVM process.
     * Pros: Low latency, simple deployment
     * Cons: No crash isolation, harder to scale
     */
    EMBEDDED,
    
    /**
     * Remote Rust storage service via gRPC - runs in separate process.
     * Pros: Crash isolation, operational flexibility, scalability
     * Cons: Higher latency due to IPC, more complex deployment
     */
    REMOTE;
    
    /**
     * Parse storage mode from string, case-insensitive.
     * 
     * @param mode String representation of storage mode
     * @return StorageMode enum value
     * @throws IllegalArgumentException if mode is invalid
     */
    public static StorageMode fromString(String mode) {
        if (mode == null || mode.trim().isEmpty()) {
            return getDefault();
        }
        
        try {
            return StorageMode.valueOf(mode.trim().toUpperCase());
        } catch (IllegalArgumentException e) {
            throw new IllegalArgumentException(
                "Invalid storage mode: '" + mode + "'. Valid options: EMBEDDED, REMOTE", e);
        }
    }
    
    /**
     * Get the default storage mode.
     * Currently defaults to REMOTE for production deployments.
     * 
     * @return Default StorageMode
     */
    public static StorageMode getDefault() {
        return REMOTE;
    }
    
    @Override
    public String toString() {
        return name().toLowerCase();
    }
} 
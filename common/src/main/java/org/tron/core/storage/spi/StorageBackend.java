package org.tron.core.storage.spi;

import java.util.Map;
import java.util.Set;

/**
 * Simplified storage backend interface for use in chainbase module.
 * This provides the essential operations needed by TronDatabase without
 * requiring the full StorageSPI interface from framework module.
 */
public interface StorageBackend {
    
    /**
     * Initialize the storage backend.
     */
    void initialize() throws Exception;
    
    /**
     * Get a value by key.
     */
    byte[] get(byte[] key) throws Exception;
    
    /**
     * Put a key-value pair.
     */
    void put(byte[] key, byte[] value) throws Exception;
    
    /**
     * Delete a key.
     */
    void delete(byte[] key) throws Exception;
    
    /**
     * Batch put multiple key-value pairs.
     */
    void batchPut(Map<byte[], byte[]> batch) throws Exception;
    
    /**
     * Check if a key exists.
     */
    boolean exists(byte[] key) throws Exception;
    
    /**
     * Get all keys.
     */
    Set<byte[]> getAllKeys() throws Exception;
    
    /**
     * Get all values.
     */
    Set<byte[]> getAllValues() throws Exception;
    
    /**
     * Get the total number of entries.
     */
    long getSize() throws Exception;
    
    /**
     * Clear all data.
     */
    void clear() throws Exception;
    
    /**
     * Flush pending writes.
     */
    void flush() throws Exception;
    
    /**
     * Close the storage backend.
     */
    void close() throws Exception;
    
    /**
     * Get storage statistics.
     */
    Map<String, String> getStats() throws Exception;
    
    /**
     * Perform a prefix scan.
     */
    Map<byte[], byte[]> prefixScan(byte[] prefix, int limit) throws Exception;
    
    /**
     * Create an iterator over all entries.
     */
    StorageIterator iterator() throws Exception;
    
    /**
     * Simple iterator interface for storage entries.
     */
    interface StorageIterator {
        boolean hasNext() throws Exception;
        Map.Entry<byte[], byte[]> next() throws Exception;
        void close() throws Exception;
    }
} 
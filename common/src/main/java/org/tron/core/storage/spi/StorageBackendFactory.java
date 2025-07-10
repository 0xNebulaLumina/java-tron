package org.tron.core.storage.spi;

/**
 * Factory interface for creating storage backends.
 * The actual implementation will be provided by the framework module.
 */
public interface StorageBackendFactory {
    
    /**
     * Create a storage backend for the specified mode.
     * 
     * @param mode The storage mode (EMBEDDED or REMOTE)
     * @param dbName The database name
     * @return A StorageBackend implementation
     * @throws Exception if creation fails
     */
    StorageBackend createStorageBackend(StorageMode mode, String dbName) throws Exception;
    
    /**
     * Get the singleton factory instance.
     * This will be set by the framework module during startup.
     */
    static StorageBackendFactory getInstance() {
        return FactoryHolder.INSTANCE;
    }
    
    /**
     * Set the factory instance (called by framework module).
     */
    static void setInstance(StorageBackendFactory factory) {
        FactoryHolder.INSTANCE = factory;
    }
    
    /**
     * Holder class for the singleton factory instance.
     */
    class FactoryHolder {
        private static volatile StorageBackendFactory INSTANCE;
    }
} 
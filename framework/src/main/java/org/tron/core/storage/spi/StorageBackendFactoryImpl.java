package org.tron.core.storage.spi;

import lombok.extern.slf4j.Slf4j;

/**
 * Implementation of StorageBackendFactory that bridges to the full StorageSPI.
 * This is initialized by the framework module and provides storage backends
 * to the chainbase module.
 */
@Slf4j(topic = "Storage")
public class StorageBackendFactoryImpl implements StorageBackendFactory {

    @Override
    public StorageBackend createStorageBackend(StorageMode mode, String dbName) throws Exception {
        // Create the full StorageSPI using the existing factory
        StorageSPI storageSPI = StorageSpiFactory.createStorageSPI(mode);
        
        // Wrap it in an adapter that implements the simplified StorageBackend interface
        return new StorageSpiBackendAdapter(storageSPI, dbName);
    }
    
    /**
     * Initialize the factory and set it as the global instance.
     * This should be called during application startup.
     */
    public static void initialize() {
        StorageBackendFactory.setInstance(new StorageBackendFactoryImpl());
        logger.info("StorageBackendFactory initialized");
    }
} 
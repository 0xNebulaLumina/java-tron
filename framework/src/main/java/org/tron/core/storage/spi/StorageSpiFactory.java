package org.tron.core.storage.spi;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Factory class for creating StorageSPI implementations based on configuration.
 * 
 * Configuration precedence (highest to lowest):
 * 1. Java system property: -Dstorage.mode=embedded|remote
 * 2. Environment variable: STORAGE_MODE=embedded|remote  
 * 3. Config file property: storage.mode=embedded|remote
 * 4. Default: REMOTE
 */
public class StorageSpiFactory {
    private static final Logger logger = LoggerFactory.getLogger(StorageSpiFactory.class);
    
    // Configuration keys
    private static final String SYSTEM_PROPERTY_KEY = "storage.mode";
    private static final String ENV_VAR_KEY = "STORAGE_MODE";
    private static final String CONFIG_FILE_KEY = "storage.mode";
    
    // gRPC configuration keys
    private static final String GRPC_HOST_PROPERTY = "storage.grpc.host";
    private static final String GRPC_PORT_PROPERTY = "storage.grpc.port";
    private static final String GRPC_HOST_ENV = "STORAGE_GRPC_HOST";
    private static final String GRPC_PORT_ENV = "STORAGE_GRPC_PORT";
    
    // Default gRPC settings
    private static final String DEFAULT_GRPC_HOST = "localhost";
    private static final int DEFAULT_GRPC_PORT = 50051;
    
    // Embedded storage settings
    private static final String EMBEDDED_BASE_PATH_PROPERTY = "storage.embedded.basePath";
    private static final String EMBEDDED_BASE_PATH_ENV = "STORAGE_EMBEDDED_BASE_PATH";
    private static final String DEFAULT_EMBEDDED_BASE_PATH = "data/rocksdb-embedded";
    
    /**
     * Create a StorageSPI implementation based on configuration.
     * 
     * @return Configured StorageSPI implementation
     * @throws RuntimeException if configuration is invalid or implementation cannot be created
     */
    public static StorageSPI createStorage() {
        StorageMode mode = determineStorageMode();
        logger.info("Creating storage implementation: {}", mode);
        
        try {
            switch (mode) {
                case EMBEDDED:
                    return createEmbeddedStorage();
                case REMOTE:
                    return createRemoteStorage();
                default:
                    throw new IllegalStateException("Unsupported storage mode: " + mode);
            }
        } catch (Exception e) {
            logger.error("Failed to create storage implementation for mode: {}", mode, e);
            throw new RuntimeException("Storage initialization failed", e);
        }
    }
    
    /**
     * Determine storage mode from configuration sources.
     * 
     * @return Configured StorageMode
     */
    public static StorageMode determineStorageMode() {
        String modeStr = null;
        
        // 1. Check system property (highest precedence)
        modeStr = System.getProperty(SYSTEM_PROPERTY_KEY);
        if (modeStr != null && !modeStr.trim().isEmpty()) {
            logger.debug("Storage mode from system property: {}", modeStr);
            return StorageMode.fromString(modeStr);
        }
        
        // 2. Check environment variable
        modeStr = System.getenv(ENV_VAR_KEY);
        if (modeStr != null && !modeStr.trim().isEmpty()) {
            logger.debug("Storage mode from environment variable: {}", modeStr);
            return StorageMode.fromString(modeStr);
        }
        
        // 3. Check config file (simplified - in real implementation would read from config)
        // For now, we'll use a system property as a placeholder
        modeStr = System.getProperty(CONFIG_FILE_KEY);
        if (modeStr != null && !modeStr.trim().isEmpty()) {
            logger.debug("Storage mode from config file: {}", modeStr);
            return StorageMode.fromString(modeStr);
        }
        
        // 4. Return default
        StorageMode defaultMode = StorageMode.getDefault();
        logger.info("Using default storage mode: {}", defaultMode);
        return defaultMode;
    }
    
    /**
     * Create embedded RocksDB storage implementation.
     * 
     * @return EmbeddedStorageSPI instance
     */
    private static StorageSPI createEmbeddedStorage() {
        String basePath = getEmbeddedBasePath();
        logger.info("Creating embedded storage with base path: {}", basePath);
        return new EmbeddedStorageSPI(basePath);
    }
    
    /**
     * Create remote gRPC storage implementation.
     * 
     * @return GrpcStorageSPI instance
     */
    private static StorageSPI createRemoteStorage() {
        String host = getGrpcHost();
        int port = getGrpcPort();
        logger.info("Creating remote storage client for {}:{}", host, port);
        return new GrpcStorageSPI(host, port);
    }
    
    /**
     * Get gRPC host from configuration.
     * 
     * @return gRPC host address
     */
    private static String getGrpcHost() {
        String host = System.getProperty(GRPC_HOST_PROPERTY);
        if (host != null && !host.trim().isEmpty()) {
            return host.trim();
        }
        
        host = System.getenv(GRPC_HOST_ENV);
        if (host != null && !host.trim().isEmpty()) {
            return host.trim();
        }
        
        return DEFAULT_GRPC_HOST;
    }
    
    /**
     * Get gRPC port from configuration.
     * 
     * @return gRPC port number
     */
    private static int getGrpcPort() {
        String portStr = System.getProperty(GRPC_PORT_PROPERTY);
        if (portStr != null && !portStr.trim().isEmpty()) {
            try {
                return Integer.parseInt(portStr.trim());
            } catch (NumberFormatException e) {
                logger.warn("Invalid gRPC port in system property: {}, using default", portStr);
            }
        }
        
        portStr = System.getenv(GRPC_PORT_ENV);
        if (portStr != null && !portStr.trim().isEmpty()) {
            try {
                return Integer.parseInt(portStr.trim());
            } catch (NumberFormatException e) {
                logger.warn("Invalid gRPC port in environment variable: {}, using default", portStr);
            }
        }
        
        return DEFAULT_GRPC_PORT;
    }
    
    /**
     * Get embedded storage base path from configuration.
     * 
     * @return Base path for embedded storage
     */
    private static String getEmbeddedBasePath() {
        String basePath = System.getProperty(EMBEDDED_BASE_PATH_PROPERTY);
        if (basePath != null && !basePath.trim().isEmpty()) {
            return basePath.trim();
        }
        
        basePath = System.getenv(EMBEDDED_BASE_PATH_ENV);
        if (basePath != null && !basePath.trim().isEmpty()) {
            return basePath.trim();
        }
        
        return DEFAULT_EMBEDDED_BASE_PATH;
    }
    
    /**
     * Get current storage mode configuration for debugging.
     * 
     * @return Current storage configuration as string
     */
    public static String getConfigurationInfo() {
        StorageMode mode = determineStorageMode();
        StringBuilder info = new StringBuilder();
        info.append("Storage Configuration:\n");
        info.append("  Mode: ").append(mode).append("\n");
        
        switch (mode) {
            case EMBEDDED:
                info.append("  Base Path: ").append(getEmbeddedBasePath()).append("\n");
                break;
            case REMOTE:
                info.append("  gRPC Host: ").append(getGrpcHost()).append("\n");
                info.append("  gRPC Port: ").append(getGrpcPort()).append("\n");
                break;
        }
        
        return info.toString();
    }
} 
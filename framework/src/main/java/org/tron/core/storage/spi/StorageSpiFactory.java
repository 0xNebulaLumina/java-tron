package org.tron.core.storage.spi;

import com.typesafe.config.Config;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.parameter.CommonParameter;

/**
 * Factory class for creating StorageSPI implementations based on configuration.
 *
 * <p>Configuration precedence (highest to lowest): 1. Java system property:
 * -Dstorage.mode=embedded|remote 2. Environment variable: STORAGE_MODE=embedded|remote 3. Config
 * file property: storage.mode=embedded|remote 4. Default: EMBEDDED
 */
public class StorageSpiFactory {
  private static final Logger logger = LoggerFactory.getLogger(StorageSpiFactory.class);

  // Configuration keys
  private static final String SYSTEM_PROPERTY_KEY = "storage.mode";
  private static final String ENV_VAR_KEY = "STORAGE_MODE";
  private static final String CONFIG_FILE_KEY = "storage.mode";

  // gRPC configuration keys
  private static final String REMOTE_HOST_PROPERTY = "storage.remote.host";
  private static final String REMOTE_PORT_PROPERTY = "storage.remote.port";
  private static final String REMOTE_HOST_ENV = "STORAGE_REMOTE_HOST";
  private static final String REMOTE_PORT_ENV = "STORAGE_REMOTE_PORT";
  private static final String REMOTE_HOST_CONFIG_KEY = "storage.remote.host";
  private static final String REMOTE_PORT_CONFIG_KEY = "storage.remote.port";

  // Default gRPC settings
  private static final String DEFAULT_REMOTE_HOST = "localhost";
  private static final int DEFAULT_REMOTE_PORT = 50011;

  // Embedded storage settings
  private static final String EMBEDDED_BASE_PATH_PROPERTY = "storage.embedded.basePath";
  private static final String EMBEDDED_BASE_PATH_ENV = "STORAGE_EMBEDDED_BASE_PATH";
  private static final String EMBEDDED_BASE_PATH_CONFIG_KEY = "storage.embedded.basePath";
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
   * Create a StorageSPI implementation for the specified mode.
   *
   * @param mode The storage mode to use (EMBEDDED or REMOTE)
   * @return Configured StorageSPI implementation
   * @throws RuntimeException if configuration is invalid or implementation cannot be created
   */
  public static StorageSPI createStorage(StorageMode mode) {
    if (mode == null) {
      throw new IllegalArgumentException("Storage mode cannot be null");
    }

    logger.info("Creating storage implementation for specified mode: {}", mode);

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

    // 3. Check config file
    modeStr = getStorageModeFromConfig();
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
   * Get storage mode from config file.
   *
   * @return Storage mode string from config, or null if not found
   */
  private static String getStorageModeFromConfig() {
    try {
      CommonParameter parameter = CommonParameter.getInstance();
      if (parameter != null) {
        // First try to get from the already loaded storage configuration
        if (parameter.getStorage() != null && parameter.getStorage().getStorageMode() != null) {
          return parameter.getStorage().getStorageMode();
        }
        
        // If not available, reload the config to get the latest values
        // This is similar to how DynamicArgs does it
        String confFileName = parameter.getShellConfFileName();
        if (confFileName == null || confFileName.trim().isEmpty()) {
          confFileName = "config.conf"; // Default config file
        }
        
        try {
          Config config = org.tron.core.config.Configuration.getByFileName(confFileName, confFileName);
          return org.tron.core.config.args.Storage.getStorageModeFromConfig(config);
        } catch (Exception e) {
          logger.debug("Could not load config file for storage mode: {}", e.getMessage());
        }
      }
    } catch (Exception e) {
      logger.debug("Could not read storage mode from config: {}", e.getMessage());
    }
    return null;
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
    String host = System.getProperty(REMOTE_HOST_PROPERTY);
    if (host != null && !host.trim().isEmpty()) {
      return host.trim();
    }

    host = System.getenv(REMOTE_HOST_ENV);
    if (host != null && !host.trim().isEmpty()) {
      return host.trim();
    }

    // Check config file
    host = getGrpcHostFromConfig();
    if (host != null && !host.trim().isEmpty()) {
      return host.trim();
    }

    return DEFAULT_REMOTE_HOST;
  }

  /**
   * Get gRPC host from config file.
   *
   * @return gRPC host from config, or null if not found
   */
  private static String getGrpcHostFromConfig() {
    try {
      // Try to get from CommonParameter config if available
      CommonParameter parameter = CommonParameter.getInstance();
      if (parameter != null) {
        // For now, we need to reload the config to get the latest values
        // This is similar to how DynamicArgs does it
        String confFileName = parameter.getShellConfFileName();
        if (confFileName == null || confFileName.trim().isEmpty()) {
          confFileName = "config.conf"; // Default config file
        }
        
        try {
          Config config = org.tron.core.config.Configuration.getByFileName(confFileName, confFileName);
          return org.tron.core.config.args.Storage.getGrpcHostFromConfig(config);
        } catch (Exception e) {
          logger.debug("Could not load config file for gRPC host: {}", e.getMessage());
        }
      }
    } catch (Exception e) {
      logger.debug("Could not read gRPC host from config: {}", e.getMessage());
    }
    return null;
  }

  /**
   * Get gRPC port from configuration.
   *
   * @return gRPC port number
   */
  private static int getGrpcPort() {
    String portStr = System.getProperty(REMOTE_PORT_PROPERTY);
    if (portStr != null && !portStr.trim().isEmpty()) {
      try {
        return Integer.parseInt(portStr.trim());
      } catch (NumberFormatException e) {
        logger.warn("Invalid gRPC port in system property: {}, using default", portStr);
      }
    }

    portStr = System.getenv(REMOTE_PORT_ENV);
    if (portStr != null && !portStr.trim().isEmpty()) {
      try {
        return Integer.parseInt(portStr.trim());
      } catch (NumberFormatException e) {
        logger.warn("Invalid gRPC port in environment variable: {}, using default", portStr);
      }
    }

    // Check config file
    Integer portFromConfig = getGrpcPortFromConfig();
    if (portFromConfig != null) {
      return portFromConfig;
    }

    return DEFAULT_REMOTE_PORT;
  }

  /**
   * Get gRPC port from config file.
   *
   * @return gRPC port from config, or null if not found
   */
  private static Integer getGrpcPortFromConfig() {
    try {
      // Try to get from CommonParameter config if available
      CommonParameter parameter = CommonParameter.getInstance();
      if (parameter != null) {
        // For now, we need to reload the config to get the latest values
        // This is similar to how DynamicArgs does it
        String confFileName = parameter.getShellConfFileName();
        if (confFileName == null || confFileName.trim().isEmpty()) {
          confFileName = "config.conf"; // Default config file
        }
        
        try {
          Config config = org.tron.core.config.Configuration.getByFileName(confFileName, confFileName);
          return org.tron.core.config.args.Storage.getGrpcPortFromConfig(config);
        } catch (Exception e) {
          logger.debug("Could not load config file for gRPC port: {}", e.getMessage());
        }
      }
    } catch (Exception e) {
      logger.debug("Could not read gRPC port from config: {}", e.getMessage());
    }
    return null;
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

    // Check config file
    basePath = getEmbeddedBasePathFromConfig();
    if (basePath != null && !basePath.trim().isEmpty()) {
      return basePath.trim();
    }

    return DEFAULT_EMBEDDED_BASE_PATH;
  }

  /**
   * Get embedded storage base path from config file.
   *
   * @return Base path from config, or null if not found
   */
  private static String getEmbeddedBasePathFromConfig() {
    try {
      // Try to get from CommonParameter config if available
      CommonParameter parameter = CommonParameter.getInstance();
      if (parameter != null) {
        // For now, we need to reload the config to get the latest values
        // This is similar to how DynamicArgs does it
        String confFileName = parameter.getShellConfFileName();
        if (confFileName == null || confFileName.trim().isEmpty()) {
          confFileName = "config.conf"; // Default config file
        }
        
        try {
          Config config = org.tron.core.config.Configuration.getByFileName(confFileName, confFileName);
          return org.tron.core.config.args.Storage.getEmbeddedBasePathFromConfig(config);
        } catch (Exception e) {
          logger.debug("Could not load config file for embedded base path: {}", e.getMessage());
        }
      }
    } catch (Exception e) {
      logger.debug("Could not read embedded base path from config: {}", e.getMessage());
    }
    return null;
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
      default:
        info.append("  Unknown mode configuration\n");
        break;
    }

    return info.toString();
  }

  // Overloaded methods that accept Config parameter for more direct config file access

  /**
   * Determine storage mode from configuration sources with explicit config.
   *
   * @param config Config object to read from
   * @return Configured StorageMode
   */
  public static StorageMode determineStorageMode(Config config) {
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

    // 3. Check config file
    if (config != null && config.hasPath(CONFIG_FILE_KEY)) {
      modeStr = config.getString(CONFIG_FILE_KEY);
      if (modeStr != null && !modeStr.trim().isEmpty()) {
        logger.debug("Storage mode from config file: {}", modeStr);
        return StorageMode.fromString(modeStr);
      }
    }

    // 4. Return default
    StorageMode defaultMode = StorageMode.getDefault();
    logger.info("Using default storage mode: {}", defaultMode);
    return defaultMode;
  }

  /**
   * Get gRPC host from configuration with explicit config.
   *
   * @param config Config object to read from
   * @return gRPC host address
   */
  public static String getGrpcHost(Config config) {
    String host = System.getProperty(REMOTE_HOST_PROPERTY);
    if (host != null && !host.trim().isEmpty()) {
      return host.trim();
    }

    host = System.getenv(REMOTE_HOST_ENV);
    if (host != null && !host.trim().isEmpty()) {
      return host.trim();
    }

    // Check config file
    if (config != null && config.hasPath(REMOTE_HOST_CONFIG_KEY)) {
      host = config.getString(REMOTE_HOST_CONFIG_KEY);
      if (host != null && !host.trim().isEmpty()) {
        return host.trim();
      }
    }

    return DEFAULT_REMOTE_HOST;
  }

  /**
   * Get gRPC port from configuration with explicit config.
   *
   * @param config Config object to read from
   * @return gRPC port number
   */
  public static int getGrpcPort(Config config) {
    String portStr = System.getProperty(REMOTE_PORT_PROPERTY);
    if (portStr != null && !portStr.trim().isEmpty()) {
      try {
        return Integer.parseInt(portStr.trim());
      } catch (NumberFormatException e) {
        logger.warn("Invalid gRPC port in system property: {}, using default", portStr);
      }
    }

    portStr = System.getenv(REMOTE_PORT_ENV);
    if (portStr != null && !portStr.trim().isEmpty()) {
      try {
        return Integer.parseInt(portStr.trim());
      } catch (NumberFormatException e) {
        logger.warn("Invalid gRPC port in environment variable: {}, using default", portStr);
      }
    }

    // Check config file
    if (config != null && config.hasPath(REMOTE_PORT_CONFIG_KEY)) {
      try {
        return config.getInt(REMOTE_PORT_CONFIG_KEY);
      } catch (Exception e) {
        logger.warn("Invalid gRPC port in config file: {}, using default", e.getMessage());
      }
    }

    return DEFAULT_REMOTE_PORT;
  }

  /**
   * Get embedded storage base path from configuration with explicit config.
   *
   * @param config Config object to read from
   * @return Base path for embedded storage
   */
  public static String getEmbeddedBasePath(Config config) {
    String basePath = System.getProperty(EMBEDDED_BASE_PATH_PROPERTY);
    if (basePath != null && !basePath.trim().isEmpty()) {
      return basePath.trim();
    }

    basePath = System.getenv(EMBEDDED_BASE_PATH_ENV);
    if (basePath != null && !basePath.trim().isEmpty()) {
      return basePath.trim();
    }

    // Check config file
    if (config != null && config.hasPath(EMBEDDED_BASE_PATH_CONFIG_KEY)) {
      basePath = config.getString(EMBEDDED_BASE_PATH_CONFIG_KEY);
      if (basePath != null && !basePath.trim().isEmpty()) {
        return basePath.trim();
      }
    }

    return DEFAULT_EMBEDDED_BASE_PATH;
  }
}

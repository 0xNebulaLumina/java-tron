package org.tron.core.execution.spi;

import com.typesafe.config.Config;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.parameter.CommonParameter;

/**
 * Factory class for creating ExecutionSPI implementations based on configuration.
 *
 * <p>This factory supports three execution modes:
 *
 * <ul>
 *   <li>EMBEDDED: Uses local Java EVM (default)
 *   <li>REMOTE: Uses remote Rust execution service
 *   <li>SHADOW: Runs both engines and compares results
 * </ul>
 *
 * <p>Configuration sources (in order of precedence):
 *
 * <ol>
 *   <li>System property: -Dexecution.mode=EMBEDDED|REMOTE|SHADOW
 *   <li>Environment variable: EXECUTION_MODE=EMBEDDED|REMOTE|SHADOW
 *   <li>Config file: execution.mode = EMBEDDED|REMOTE|SHADOW
 *   <li>Default: EMBEDDED
 * </ol>
 */
public class ExecutionSpiFactory {
  private static final Logger logger = LoggerFactory.getLogger(ExecutionSpiFactory.class);

  // Configuration keys
  private static final String SYSTEM_PROPERTY_KEY = "execution.mode";
  private static final String ENV_VAR_KEY = "EXECUTION_MODE";
  private static final String CONFIG_FILE_KEY = "execution.mode";

  // Remote service configuration
  private static final String REMOTE_HOST_SYSTEM_PROPERTY = "execution.remote.host";
  private static final String REMOTE_HOST_ENV_VAR = "EXECUTION_REMOTE_HOST";
  private static final String REMOTE_HOST_CONFIG = "execution.remote.host";
  private static final String DEFAULT_REMOTE_HOST = "127.0.0.1";

  private static final String REMOTE_PORT_SYSTEM_PROPERTY = "execution.remote.port";
  private static final String REMOTE_PORT_ENV_VAR = "EXECUTION_REMOTE_PORT";
  private static final String REMOTE_PORT_CONFIG = "execution.remote.port";
  private static final int DEFAULT_REMOTE_PORT = 50012;

  /**
   * Create an ExecutionSPI implementation based on configuration.
   *
   * @return Configured ExecutionSPI implementation
   * @throws RuntimeException if configuration is invalid or implementation cannot be created
   */
  public static ExecutionSPI createExecution() {
    ExecutionMode mode = determineExecutionMode();
    logger.info("Creating execution implementation: {}", mode);

    try {
      switch (mode) {
        case EMBEDDED:
          return createEmbeddedExecution();
        case REMOTE:
          return createRemoteExecution();
        case SHADOW:
          return createShadowExecution();
        default:
          throw new IllegalStateException("Unsupported execution mode: " + mode);
      }
    } catch (Exception e) {
      logger.error("Failed to create execution implementation for mode: {}", mode, e);
      throw new RuntimeException("Execution initialization failed", e);
    }
  }

  /**
   * Create an ExecutionSPI implementation for the specified mode.
   *
   * @param mode The execution mode to use (EMBEDDED, REMOTE, or SHADOW)
   * @return Configured ExecutionSPI implementation
   * @throws RuntimeException if configuration is invalid or implementation cannot be created
   */
  public static ExecutionSPI createExecution(ExecutionMode mode) {
    if (mode == null) {
      throw new IllegalArgumentException("Execution mode cannot be null");
    }

    logger.info("Creating execution implementation for specified mode: {}", mode);

    try {
      switch (mode) {
        case EMBEDDED:
          return createEmbeddedExecution();
        case REMOTE:
          return createRemoteExecution();
        case SHADOW:
          return createShadowExecution();
        default:
          throw new IllegalStateException("Unsupported execution mode: " + mode);
      }
    } catch (Exception e) {
      logger.error("Failed to create execution implementation for mode: {}", mode, e);
      throw new RuntimeException("Execution initialization failed", e);
    }
  }

  /**
   * Determine execution mode from configuration sources.
   *
   * @return Configured ExecutionMode
   */
  public static ExecutionMode determineExecutionMode() {
    String modeStr = null;

    // 1. Check system property (highest precedence)
    modeStr = System.getProperty(SYSTEM_PROPERTY_KEY);
    if (modeStr != null && !modeStr.trim().isEmpty()) {
      logger.debug("Execution mode from system property: {}", modeStr);
      return ExecutionMode.fromString(modeStr);
    }

    // 2. Check environment variable
    modeStr = System.getenv(ENV_VAR_KEY);
    if (modeStr != null && !modeStr.trim().isEmpty()) {
      logger.debug("Execution mode from environment variable: {}", modeStr);
      return ExecutionMode.fromString(modeStr);
    }

    // 3. Check config file
    Config config = getConfig();
    if (config != null) {
      modeStr = getExecutionModeFromConfig(config);
      if (modeStr != null && !modeStr.trim().isEmpty()) {
        logger.debug("Execution mode from config file: {}", modeStr);
        return ExecutionMode.fromString(modeStr);
      }
    }

    // 4. Return default
    ExecutionMode defaultMode = ExecutionMode.getDefault();
    logger.info("Using default execution mode: {}", defaultMode);
    return defaultMode;
  }

  /**
   * Determine execution mode from configuration sources with explicit config.
   *
   * @param config Config object to read from
   * @return Configured ExecutionMode
   */
  public static ExecutionMode determineExecutionMode(Config config) {
    String modeStr = null;

    // 1. Check system property (highest precedence)
    modeStr = System.getProperty(SYSTEM_PROPERTY_KEY);
    if (modeStr != null && !modeStr.trim().isEmpty()) {
      logger.debug("Execution mode from system property: {}", modeStr);
      return ExecutionMode.fromString(modeStr);
    }

    // 2. Check environment variable
    modeStr = System.getenv(ENV_VAR_KEY);
    if (modeStr != null && !modeStr.trim().isEmpty()) {
      logger.debug("Execution mode from environment variable: {}", modeStr);
      return ExecutionMode.fromString(modeStr);
    }

    // 3. Check config file
    if (config != null && config.hasPath(CONFIG_FILE_KEY)) {
      modeStr = config.getString(CONFIG_FILE_KEY);
      if (modeStr != null && !modeStr.trim().isEmpty()) {
        logger.debug("Execution mode from config file: {}", modeStr);
        return ExecutionMode.fromString(modeStr);
      }
    }

    // 4. Return default
    ExecutionMode defaultMode = ExecutionMode.getDefault();
    logger.info("Using default execution mode: {}", defaultMode);
    return defaultMode;
  }

  /**
   * Get remote host from configuration.
   *
   * @return Remote host
   */
  public static String getRemoteHost() {
    // 1. Check system property
    String host = System.getProperty(REMOTE_HOST_SYSTEM_PROPERTY);
    if (host != null && !host.trim().isEmpty()) {
      logger.debug("Remote host from system property: {}", host);
      return host;
    }

    // 2. Check environment variable
    host = System.getenv(REMOTE_HOST_ENV_VAR);
    if (host != null && !host.trim().isEmpty()) {
      logger.debug("Remote host from environment variable: {}", host);
      return host;
    }

    // 3. Check config file
    Config config = getConfig();
    if (config != null) {
      host = getRemoteHost(config);
      if (host != null && !host.trim().isEmpty() && !host.equals(DEFAULT_REMOTE_HOST)) {
        return host;
      }
    }

    // 4. Return default
    logger.debug("Using default remote host: {}", DEFAULT_REMOTE_HOST);
    return DEFAULT_REMOTE_HOST;
  }

  /**
   * Get remote host from explicit config.
   *
   * @param config Config object to read from
   * @return Remote host
   */
  public static String getRemoteHost(Config config) {
    if (config != null && config.hasPath(REMOTE_HOST_CONFIG)) {
      String host = config.getString(REMOTE_HOST_CONFIG);
      logger.debug("Remote host from config file: {}", host);
      return host;
    }
    return DEFAULT_REMOTE_HOST;
  }

  /**
   * Get remote port from configuration.
   *
   * @return Remote port
   */
  public static int getRemotePort() {
    // 1. Check system property
    String portStr = System.getProperty(REMOTE_PORT_SYSTEM_PROPERTY);
    if (portStr != null && !portStr.trim().isEmpty()) {
      try {
        int port = Integer.parseInt(portStr);
        logger.debug("Remote port from system property: {}", port);
        return port;
      } catch (NumberFormatException e) {
        logger.warn("Invalid remote port in system property: {}", portStr);
      }
    }

    // 2. Check environment variable
    portStr = System.getenv(REMOTE_PORT_ENV_VAR);
    if (portStr != null && !portStr.trim().isEmpty()) {
      try {
        int port = Integer.parseInt(portStr);
        logger.debug("Remote port from environment variable: {}", port);
        return port;
      } catch (NumberFormatException e) {
        logger.warn("Invalid remote port in environment variable: {}", portStr);
      }
    }

    // 3. Check config file
    Config config = getConfig();
    if (config != null) {
      int port = getRemotePort(config);
      if (port != DEFAULT_REMOTE_PORT) {
        return port;
      }
    }

    // 4. Return default
    logger.debug("Using default remote port: {}", DEFAULT_REMOTE_PORT);
    return DEFAULT_REMOTE_PORT;
  }

  /**
   * Get remote port from explicit config.
   *
   * @param config Config object to read from
   * @return Remote port
   */
  public static int getRemotePort(Config config) {
    if (config != null && config.hasPath(REMOTE_PORT_CONFIG)) {
      int port = config.getInt(REMOTE_PORT_CONFIG);
      logger.debug("Remote port from config file: {}", port);
      return port;
    }
    return DEFAULT_REMOTE_PORT;
  }

  /**
   * Get configuration information as a string.
   *
   * @return Configuration information
   */
  public static String getConfigurationInfo() {
    StringBuilder sb = new StringBuilder();
    sb.append("Execution Configuration:\n");
    sb.append("  Mode: ").append(determineExecutionMode()).append("\n");
    sb.append("  Remote Host: ").append(getRemoteHost()).append("\n");
    sb.append("  Remote Port: ").append(getRemotePort()).append("\n");
    return sb.toString();
  }

  /**
   * Get config object from common parameter.
   *
   * @return Config object or null if not available
   */
  private static Config getConfig() {
    try {
      CommonParameter parameter = CommonParameter.getInstance();
      if (parameter != null) {
        String confFileName = parameter.getShellConfFileName();
        if (confFileName != null && !confFileName.trim().isEmpty()) {
          try {
            Config config =
                org.tron.core.config.Configuration.getByFileName(confFileName, confFileName);
            return config;
          } catch (Exception e) {
            logger.debug("Could not load config file for execution mode: {}", e.getMessage());
          }
        }
      }
    } catch (Exception e) {
      logger.debug("Could not read execution mode from config: {}", e.getMessage());
    }
    return null;
  }

  /**
   * Get execution mode from config.
   *
   * @param config Config object to read from
   * @return Execution mode string or null if not available
   */
  private static String getExecutionModeFromConfig(Config config) {
    if (config != null && config.hasPath(CONFIG_FILE_KEY)) {
      return config.getString(CONFIG_FILE_KEY);
    }
    return null;
  }

  /**
   * Create embedded Java EVM implementation.
   *
   * @return EmbeddedExecutionSPI instance
   */
  private static ExecutionSPI createEmbeddedExecution() {
    logger.info("Creating embedded execution implementation");
    return new EmbeddedExecutionSPI();
  }

  /**
   * Create remote Rust execution implementation.
   *
   * @return RemoteExecutionSPI instance
   */
  private static ExecutionSPI createRemoteExecution() {
    String host = getRemoteHost();
    int port = getRemotePort();
    logger.info("Creating remote execution implementation with host: {}:{}", host, port);
    return new RemoteExecutionSPI(host, port);
  }

  /**
   * Create shadow execution implementation that runs both engines.
   *
   * @return ShadowExecutionSPI instance
   */
  private static ExecutionSPI createShadowExecution() {
    logger.info("Creating shadow execution implementation");
    ExecutionSPI embedded = createEmbeddedExecution();
    ExecutionSPI remote = createRemoteExecution();
    return new ShadowExecutionSPI(embedded, remote);
  }
}

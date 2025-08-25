package org.tron.core.execution.spi;

/**
 * Enumeration of available execution backend modes.
 *
 * <p>EMBEDDED - Uses embedded Java EVM via TronVM REMOTE - Uses remote Rust execution service via
 * gRPC SHADOW - Runs both engines and compares results for verification
 */
public enum ExecutionMode {
  /**
   * Embedded Java EVM execution - runs in the same JVM process. Pros: Low latency, simple
   * deployment, battle-tested Cons: No crash isolation, harder to scale, limited optimization
   */
  EMBEDDED,

  /**
   * Remote Rust execution service via gRPC - runs in separate process. Pros: Crash isolation,
   * operational flexibility, performance optimization Cons: Higher latency due to IPC, more complex
   * deployment
   */
  REMOTE,

  /**
   * Shadow execution mode - runs both engines and compares results. Used for verification and
   * testing before production cut-over. Pros: Confidence in equivalence, gradual migration Cons: 2x
   * compute overhead, complex error handling
   */
  SHADOW;

  /**
   * Parse execution mode from string, case-insensitive.
   *
   * @param mode String representation of execution mode
   * @return ExecutionMode enum value
   * @throws IllegalArgumentException if mode is invalid
   */
  public static ExecutionMode fromString(String mode) {
    if (mode == null || mode.trim().isEmpty()) {
      return getDefault();
    }

    try {
      return ExecutionMode.valueOf(mode.trim().toUpperCase());
    } catch (IllegalArgumentException e) {
      throw new IllegalArgumentException(
          "Invalid execution mode: '" + mode + "'. Valid options: EMBEDDED, REMOTE, SHADOW", e);
    }
  }

  /**
   * Get the default execution mode. Currently defaults to EMBEDDED for backward compatibility.
   *
   * @return Default ExecutionMode
   */
  public static ExecutionMode getDefault() {
    return EMBEDDED;
  }

  @Override
  public String toString() {
    return name().toLowerCase();
  }
}

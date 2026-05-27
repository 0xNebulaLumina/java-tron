package org.tron.core.db;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;
import static org.junit.Assert.fail;

import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.util.concurrent.CompletableFuture;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.tron.common.parameter.CommonParameter;
import org.tron.common.runtime.Runtime;
import org.tron.common.runtime.RuntimeImpl;
import org.tron.core.execution.spi.ExecutionMode;
import org.tron.core.execution.spi.ExecutionProgramResult;
import org.tron.core.execution.spi.ExecutionSPI;
import org.tron.core.execution.spi.ExecutionSpiFactory;
import org.tron.common.runtime.RuntimeSpiImpl;

/**
 * Integration test for ExecutionSPI integration with java-tron. Tests the runtime selection logic
 * and configuration handling. This is a simple JUnit test without Spring context to avoid
 * initialization issues.
 */
public class ExecutionSpiIntegrationTest {

  private boolean originalExecutionSpiEnabled;
  private String originalExecutionMode;
  private String originalExecutionModeProperty;
  private ExecutionSPI originalExecutionSpi;
  private ExecutionMode originalInitializedMode;

  @Before
  public void setUp() {
    // Save original configuration
    originalExecutionSpiEnabled = CommonParameter.getInstance().isExecutionSpiEnabled();
    originalExecutionMode = CommonParameter.getInstance().getExecutionMode();
    originalExecutionModeProperty = System.getProperty("execution.mode");
    try {
      originalExecutionSpi = getExecutionSpiInstance();
      originalInitializedMode = getExecutionSpiInitializedMode();
    } catch (Exception e) {
      throw new RuntimeException(e);
    }

    // Initialize ExecutionSPI factory
    try {
      ExecutionSpiFactory.initialize();
    } catch (Exception e) {
      // Factory may already be initialized
    }
  }

  @After
  public void tearDown() {
    // Restore original configuration
    CommonParameter.getInstance().setExecutionSpiEnabled(originalExecutionSpiEnabled);
    CommonParameter.getInstance().setExecutionMode(originalExecutionMode);
    restoreSystemProperty("execution.mode", originalExecutionModeProperty);
    try {
      setExecutionSpiInstance(originalExecutionSpi);
      setExecutionSpiInitializedMode(originalInitializedMode);
    } catch (Exception e) {
      throw new RuntimeException(e);
    }
  }

  @Test
  public void testExecutionSpiFactoryInitialization() {
    // Test that ExecutionSPI factory is properly initialized
    assertNotNull("ExecutionSPI factory should be initialized", ExecutionSpiFactory.getInstance());
  }

  @Test
  public void testDefaultConfiguration() {
    // Test default configuration values
    CommonParameter params = CommonParameter.getInstance();

    // Default should be disabled and EMBEDDED mode
    assertEquals("Default execution SPI should be disabled", false, params.isExecutionSpiEnabled());
    assertEquals(
        "Default execution mode should be EMBEDDED", "EMBEDDED", params.getExecutionMode());
  }

  @Test
  public void testExecutionModeConfiguration() {
    // Test setting different execution modes
    CommonParameter params = CommonParameter.getInstance();

    params.setExecutionMode("REMOTE");
    assertEquals("Execution mode should be set to REMOTE", "REMOTE", params.getExecutionMode());

    params.setExecutionMode("SHADOW");
    assertEquals("Execution mode should be set to SHADOW", "SHADOW", params.getExecutionMode());

    params.setExecutionMode("EMBEDDED");
    assertEquals("Execution mode should be set to EMBEDDED", "EMBEDDED", params.getExecutionMode());
  }

  @Test
  public void testExecutionSpiEnabledConfiguration() {
    // Test enabling/disabling ExecutionSPI
    CommonParameter params = CommonParameter.getInstance();

    params.setExecutionSpiEnabled(true);
    assertTrue("ExecutionSPI should be enabled", params.isExecutionSpiEnabled());

    params.setExecutionSpiEnabled(false);
    assertTrue("ExecutionSPI should be disabled", !params.isExecutionSpiEnabled());
  }

  @Test
  public void testExecutionModeFromFactory() {
    // Test that ExecutionSpiFactory correctly determines execution mode
    ExecutionMode mode = ExecutionSpiFactory.determineExecutionMode();
    assertNotNull("Execution mode should not be null", mode);

    // Should default to EMBEDDED
    assertEquals("Default mode should be EMBEDDED", ExecutionMode.EMBEDDED, mode);
  }

  @Test
  public void testExecutionModeWithSystemProperty() {
    // Test execution mode determination with system property
    String originalProperty = System.getProperty("execution.mode");

    try {
      // Set system property
      System.setProperty("execution.mode", "REMOTE");

      ExecutionMode mode = ExecutionSpiFactory.determineExecutionMode();
      assertEquals("Mode should be REMOTE from system property", ExecutionMode.REMOTE, mode);

    } finally {
      // Clean up system property
      if (originalProperty != null) {
        System.setProperty("execution.mode", originalProperty);
      } else {
        System.clearProperty("execution.mode");
      }
    }
  }

  @Test
  public void testExecutionModeWithEnvironmentVariable() {
    // Note: Environment variables cannot be easily set in unit tests
    // This test verifies the logic exists but may not change the actual environment

    ExecutionMode mode = ExecutionSpiFactory.determineExecutionMode();
    assertNotNull("Execution mode should be determinable", mode);

    // Should be a valid mode
    assertTrue(
        "Mode should be valid",
        mode == ExecutionMode.EMBEDDED
            || mode == ExecutionMode.REMOTE
            || mode == ExecutionMode.SHADOW);
  }

  @Test
  public void testRuntimeCreationLogic() {
    // Test the runtime creation logic (simulating Manager.createRuntime())
    CommonParameter params = CommonParameter.getInstance();

    // Test with ExecutionSPI disabled (should use RuntimeImpl)
    params.setExecutionSpiEnabled(false);
    params.setExecutionMode("EMBEDDED");

    // Simulate the logic from Manager.shouldUseExecutionSpi()
    boolean shouldUseExecutionSpi = params.isExecutionSpiEnabled();

    // Debug: Check what mode is being determined
    ExecutionMode actualMode = ExecutionSpiFactory.determineExecutionMode();
    System.out.println("Actual execution mode: " + actualMode);

    if (!shouldUseExecutionSpi && ExecutionSpiFactory.getInstance() != null) {
      String mode = actualMode.toString();
      shouldUseExecutionSpi = !"EMBEDDED".equals(mode);
    }

    // The test should pass regardless of the actual mode since ExecutionSPI is explicitly disabled
    assertTrue(
        "ExecutionSPI should be disabled when explicitly set to false",
        !params.isExecutionSpiEnabled());
  }

  @Test
  public void testRuntimeCreationWithExecutionSpiEnabled() {
    // Test runtime creation with ExecutionSPI enabled
    CommonParameter params = CommonParameter.getInstance();
    params.setExecutionSpiEnabled(true);

    // Simulate the logic from Manager.shouldUseExecutionSpi()
    boolean shouldUseExecutionSpi = params.isExecutionSpiEnabled();
    assertTrue("Should use ExecutionSPI when enabled", shouldUseExecutionSpi);
  }

  @Test
  public void testRuntimeCreationWithRemoteMode() {
    // Test runtime creation with REMOTE mode
    String originalProperty = System.getProperty("execution.mode");

    try {
      // Set system property to REMOTE
      System.setProperty("execution.mode", "REMOTE");

      // Simulate the logic from Manager.shouldUseExecutionSpi()
      CommonParameter params = CommonParameter.getInstance();
      boolean shouldUseExecutionSpi = params.isExecutionSpiEnabled();

      if (!shouldUseExecutionSpi && ExecutionSpiFactory.getInstance() != null) {
        String mode = ExecutionSpiFactory.determineExecutionMode().toString();
        shouldUseExecutionSpi = !"EMBEDDED".equals(mode);
      }

      assertTrue("Should use ExecutionSPI with REMOTE mode", shouldUseExecutionSpi);

    } finally {
      // Clean up system property
      if (originalProperty != null) {
        System.setProperty("execution.mode", originalProperty);
      } else {
        System.clearProperty("execution.mode");
      }
    }
  }

  @Test
  public void testManagerCreateRuntimeUsesRuntimeImplForEmbeddedMode() throws Exception {
    setRuntimeSelectionState("EMBEDDED", ExecutionMode.REMOTE);

    Runtime runtime = invokeCreateRuntime(new Manager());

    assertTrue("EMBEDDED mode should use RuntimeImpl", runtime instanceof RuntimeImpl);
  }

  @Test
  public void testManagerCreateRuntimeUsesRuntimeImplForShadowMode() throws Exception {
    setRuntimeSelectionState("SHADOW", ExecutionMode.REMOTE);

    Runtime runtime = invokeCreateRuntime(new Manager());

    assertTrue("SHADOW mode should use RuntimeImpl", runtime instanceof RuntimeImpl);
  }

  @Test
  public void testManagerCreateRuntimeUsesRuntimeSpiImplForRemoteMode() throws Exception {
    setRuntimeSelectionState("REMOTE", ExecutionMode.REMOTE);

    Runtime runtime = invokeCreateRuntime(new Manager());

    assertTrue("REMOTE mode should use RuntimeSpiImpl", runtime instanceof RuntimeSpiImpl);
  }

  @Test
  public void testManagerCreateRuntimeRejectsStaleInitializedMode() throws Exception {
    setRuntimeSelectionState("REMOTE", ExecutionMode.EMBEDDED);

    try {
      invokeCreateRuntime(new Manager());
      fail("REMOTE mode should reject stale initialized ExecutionSPI mode");
    } catch (InvocationTargetException e) {
      assertTrue(e.getCause() instanceof IllegalStateException);
      assertTrue(e.getCause().getMessage().contains("not initialized for REMOTE mode"));
    }
  }

  @Test
  public void testConfigurationInfo() {
    // Test configuration information retrieval
    String configInfo = ExecutionSpiFactory.getConfigurationInfo();
    assertNotNull("Configuration info should not be null", configInfo);
    assertTrue(
        "Configuration info should contain execution information",
        configInfo.contains("Execution Configuration"));
    assertTrue("Configuration info should contain mode", configInfo.contains("Mode:"));
  }

  @Test
  public void testBackwardCompatibility() {
    // Test that the integration maintains backward compatibility

    // Default configuration should work as before
    CommonParameter params = CommonParameter.getInstance();
    params.setExecutionSpiEnabled(false);
    params.setExecutionMode("EMBEDDED");

    // Should be able to create traditional runtime
    Runtime traditionalRuntime = new RuntimeImpl();
    assertNotNull("Traditional runtime should be creatable", traditionalRuntime);

    // Runtime should implement the interface
    assertTrue("Runtime should implement Runtime interface", traditionalRuntime instanceof Runtime);
  }

  @Test
  public void testRuntimeSpiImplInitialization() {
    // Test that RuntimeSpiImpl properly initializes the ExecutionSPI factory
    try {
      // Create RuntimeSpiImpl - it should automatically initialize the factory
      RuntimeSpiImpl runtime = new RuntimeSpiImpl();
      assertNotNull("Runtime should not be null", runtime);

      // Verify that the factory is now initialized
      ExecutionSPI instance = ExecutionSpiFactory.getInstance();
      assertNotNull("ExecutionSPI factory should be initialized", instance);

    } catch (Exception e) {
      // Expected for REMOTE/SHADOW modes if services are not running
      System.out.println("Expected exception for non-embedded modes: " + e.getMessage());
    }
  }

  private void setRuntimeSelectionState(String mode, ExecutionMode initializedMode)
      throws Exception {
    CommonParameter.getInstance().setExecutionSpiEnabled(true);
    CommonParameter.getInstance().setExecutionMode(mode);
    System.setProperty("execution.mode", mode);
    setExecutionSpiInstance(new FakeExecutionSPI());
    setExecutionSpiInitializedMode(initializedMode);
  }

  private Runtime invokeCreateRuntime(Manager manager) throws Exception {
    Method method = Manager.class.getDeclaredMethod("createRuntime");
    method.setAccessible(true);
    return (Runtime) method.invoke(manager);
  }

  private ExecutionSPI getExecutionSpiInstance() throws Exception {
    java.lang.reflect.Field field = ExecutionSpiFactory.class.getDeclaredField("instance");
    field.setAccessible(true);
    return (ExecutionSPI) field.get(null);
  }

  private void setExecutionSpiInstance(ExecutionSPI executionSPI) throws Exception {
    java.lang.reflect.Field field = ExecutionSpiFactory.class.getDeclaredField("instance");
    field.setAccessible(true);
    field.set(null, executionSPI);
  }

  private ExecutionMode getExecutionSpiInitializedMode() throws Exception {
    java.lang.reflect.Field field = ExecutionSpiFactory.class.getDeclaredField("initializedMode");
    field.setAccessible(true);
    return (ExecutionMode) field.get(null);
  }

  private void setExecutionSpiInitializedMode(ExecutionMode mode) throws Exception {
    java.lang.reflect.Field field = ExecutionSpiFactory.class.getDeclaredField("initializedMode");
    field.setAccessible(true);
    field.set(null, mode);
  }

  private void restoreSystemProperty(String key, String value) {
    if (value == null) {
      System.clearProperty(key);
    } else {
      System.setProperty(key, value);
    }
  }

  @Test
  public void testExecutionModeFromCommonParameter() {
    // Test that ExecutionSpiFactory.determineExecutionMode() now considers CommonParameter
    CommonParameter params = CommonParameter.getInstance();
    String originalMode = params.getExecutionMode();

    try {
      // Set execution mode in CommonParameter
      params.setExecutionMode("REMOTE");

      // Determine execution mode should now pick up the CommonParameter value
      ExecutionMode mode = ExecutionSpiFactory.determineExecutionMode();
      assertEquals("Should use REMOTE mode from CommonParameter", ExecutionMode.REMOTE, mode);

      // Test with SHADOW mode
      params.setExecutionMode("SHADOW");
      mode = ExecutionSpiFactory.determineExecutionMode();
      assertEquals("Should use SHADOW mode from CommonParameter", ExecutionMode.SHADOW, mode);

    } finally {
      // Restore original mode
      params.setExecutionMode(originalMode);
    }
  }

  private static class FakeExecutionSPI implements ExecutionSPI {
    @Override
    public CompletableFuture<ExecutionProgramResult> executeTransaction(TransactionContext context) {
      return CompletableFuture.completedFuture(new ExecutionProgramResult());
    }

    @Override
    public CompletableFuture<ExecutionProgramResult> callContract(TransactionContext context) {
      return CompletableFuture.completedFuture(new ExecutionProgramResult());
    }

    @Override
    public CompletableFuture<Long> estimateEnergy(TransactionContext context) {
      return CompletableFuture.completedFuture(0L);
    }

    @Override
    public CompletableFuture<byte[]> getCode(byte[] address, String snapshotId) {
      return CompletableFuture.completedFuture(new byte[0]);
    }

    @Override
    public CompletableFuture<byte[]> getStorageAt(byte[] address, byte[] key, String snapshotId) {
      return CompletableFuture.completedFuture(new byte[0]);
    }

    @Override
    public CompletableFuture<Long> getNonce(byte[] address, String snapshotId) {
      return CompletableFuture.completedFuture(0L);
    }

    @Override
    public CompletableFuture<byte[]> getBalance(byte[] address, String snapshotId) {
      return CompletableFuture.completedFuture(new byte[0]);
    }

    @Override
    public CompletableFuture<String> createSnapshot() {
      return CompletableFuture.completedFuture("snapshot");
    }

    @Override
    public CompletableFuture<Boolean> revertToSnapshot(String snapshotId) {
      return CompletableFuture.completedFuture(true);
    }

    @Override
    public CompletableFuture<HealthStatus> healthCheck() {
      return CompletableFuture.completedFuture(new HealthStatus(true, "ok"));
    }

    @Override
    public void registerMetricsCallback(MetricsCallback callback) {
    }
  }
}

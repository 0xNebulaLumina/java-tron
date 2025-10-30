package org.tron.common.runtime;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

import org.junit.Before;
import org.junit.Test;
import org.tron.core.execution.spi.ExecutionMode;
import org.tron.core.execution.spi.ExecutionSpiFactory;

/**
 * Test class for RuntimeSpiImpl to verify ExecutionSPI integration. This is a simple JUnit test
 * without Spring context to avoid initialization issues.
 */
public class RuntimeSpiImplTest {

  @Before
  public void setUp() {
    // Initialize ExecutionSPI factory for testing
    try {
      ExecutionSpiFactory.initialize();
    } catch (Exception e) {
      // Factory may already be initialized
    }
  }

  @Test
  public void testExecutionModeDetection() {
    // Test that execution mode can be determined
    ExecutionMode mode = ExecutionSpiFactory.determineExecutionMode();
    assertNotNull("Execution mode should not be null", mode);

    // Default mode should be EMBEDDED
    assertEquals("Default execution mode should be EMBEDDED", ExecutionMode.EMBEDDED, mode);
  }

  @Test
  public void testExecutionSpiFactoryInitialization() {
    // Test that ExecutionSPI factory is properly initialized
    assertNotNull("ExecutionSPI instance should be available", ExecutionSpiFactory.getInstance());
  }

  @Test
  public void testConfigurationInfo() {
    // Test that configuration information can be retrieved
    String configInfo = ExecutionSpiFactory.getConfigurationInfo();
    assertNotNull("Configuration info should not be null", configInfo);
    assertTrue("Configuration info should contain mode information", configInfo.contains("Mode:"));
  }

  @Test
  public void testExecutionModeFromString() {
    // Test ExecutionMode enum parsing
    assertEquals(
        "EMBEDDED mode should parse correctly",
        ExecutionMode.EMBEDDED,
        ExecutionMode.fromString("EMBEDDED"));
    assertEquals(
        "REMOTE mode should parse correctly",
        ExecutionMode.REMOTE,
        ExecutionMode.fromString("REMOTE"));
    assertEquals(
        "SHADOW mode should parse correctly",
        ExecutionMode.SHADOW,
        ExecutionMode.fromString("SHADOW"));

    // Test case insensitive parsing
    assertEquals(
        "Lowercase embedded should parse correctly",
        ExecutionMode.EMBEDDED,
        ExecutionMode.fromString("embedded"));
  }

  @Test
  public void testDefaultExecutionMode() {
    // Test that default execution mode is EMBEDDED
    ExecutionMode defaultMode = ExecutionMode.getDefault();
    assertEquals("Default execution mode should be EMBEDDED", ExecutionMode.EMBEDDED, defaultMode);
  }

  @Test
  public void testTrc10ApplyToggle() {
    // Test that TRC-10 apply toggle can be read
    // Default should be true (apply enabled)
    String defaultValue = System.getProperty("remote.exec.apply.trc10", "true");
    assertEquals("Default TRC-10 apply toggle should be true", "true", defaultValue);
  }

  @Test
  public void testTrc10MappingToggle() {
    // Test that TRC-10 mapping toggle can be read
    // Default should be false (mapping disabled)
    String defaultValue = System.getProperty("remote.exec.trc10.enabled", "false");
    assertEquals("Default TRC-10 mapping toggle should be false", "false", defaultValue);
  }

  @Test
  public void testTrc10LedgerChangeDTO() {
    // Test TRC-10 DTO creation
    org.tron.core.execution.spi.ExecutionSPI.Trc10Op op =
        org.tron.core.execution.spi.ExecutionSPI.Trc10Op.ISSUE;
    assertEquals("ISSUE op should have value 0", 0, op.getValue());

    org.tron.core.execution.spi.ExecutionSPI.Trc10Op participate =
        org.tron.core.execution.spi.ExecutionSPI.Trc10Op.PARTICIPATE;
    assertEquals("PARTICIPATE op should have value 1", 1, participate.getValue());

    // Test fromValue
    assertEquals("fromValue(0) should return ISSUE",
        org.tron.core.execution.spi.ExecutionSPI.Trc10Op.ISSUE,
        org.tron.core.execution.spi.ExecutionSPI.Trc10Op.fromValue(0));
  }

  @Test
  public void testFrozenSupplyDTO() {
    // Test FrozenSupply DTO
    org.tron.core.execution.spi.ExecutionSPI.FrozenSupply frozen =
        new org.tron.core.execution.spi.ExecutionSPI.FrozenSupply(1000000L, 10L);

    assertEquals("Frozen amount should be 1000000", 1000000L, frozen.getFrozenAmount());
    assertEquals("Frozen days should be 10", 10L, frozen.getFrozenDays());
  }

  @Test
  public void testTrc10LedgerChangeCreation() {
    // Test complete Trc10LedgerChange DTO creation
    byte[] ownerAddr = new byte[21];
    ownerAddr[0] = 0x41;
    byte[] toAddr = new byte[21];
    toAddr[0] = 0x41;

    java.util.List<org.tron.core.execution.spi.ExecutionSPI.FrozenSupply> frozenList =
        new java.util.ArrayList<>();
    frozenList.add(new org.tron.core.execution.spi.ExecutionSPI.FrozenSupply(1000000L, 10L));

    org.tron.core.execution.spi.ExecutionSPI.Trc10LedgerChange change =
        new org.tron.core.execution.spi.ExecutionSPI.Trc10LedgerChange(
            org.tron.core.execution.spi.ExecutionSPI.Trc10Op.ISSUE,
            ownerAddr,
            toAddr,
            "1000001".getBytes(), // assetId bytes
            0L, // amount
            "TestToken".getBytes(), // name bytes
            "TT".getBytes(), // abbr bytes
            10000000L, // totalSupply
            6, // precision
            frozenList,
            1, // trxNum
            1, // num
            System.currentTimeMillis(), // startTime
            System.currentTimeMillis() + 86400000L, // endTime
            "Test token description".getBytes(), // description bytes
            "https://test.com".getBytes(), // url bytes
            0L, // freeAssetNetLimit
            0L, // publicFreeAssetNetLimit
            1024000000L // feeSun
        );

    assertNotNull("Trc10LedgerChange should be created", change);
    assertEquals("Op should be ISSUE",
        org.tron.core.execution.spi.ExecutionSPI.Trc10Op.ISSUE, change.getOp());
    assertTrue("Name should equal TestToken bytes",
        java.util.Arrays.equals("TestToken".getBytes(), change.getName()));
    assertEquals("Total supply should be 10000000", 10000000L, change.getTotalSupply());
    assertEquals("Precision should be 6", 6, change.getPrecision());
    assertEquals("Frozen supply list should have 1 entry", 1, change.getFrozenSupply().size());
  }

  @Test
  public void testExecutionResultWithTrc10Changes() {
    // Test ExecutionResult with trc10Changes field
    java.util.List<org.tron.core.execution.spi.ExecutionSPI.Trc10LedgerChange> trc10Changes =
        new java.util.ArrayList<>();

    org.tron.core.execution.spi.ExecutionSPI.ExecutionResult result =
        new org.tron.core.execution.spi.ExecutionSPI.ExecutionResult(
            true, // success
            new byte[0], // returnData
            0L, // energyUsed
            0L, // energyRefunded
            new java.util.ArrayList<>(), // stateChanges
            new java.util.ArrayList<>(), // logs
            null, // errorMessage
            0L, // bandwidthUsed
            new java.util.ArrayList<>(), // freezeChanges
            new java.util.ArrayList<>(), // globalResourceChanges
            trc10Changes // trc10Changes
        );

    assertNotNull("ExecutionResult should be created", result);
    assertNotNull("TRC-10 changes list should not be null", result.getTrc10Changes());
    assertEquals("TRC-10 changes list should be empty", 0, result.getTrc10Changes().size());
  }

  @Test
  public void testExecutionProgramResultWithTrc10Changes() {
    // Test ExecutionProgramResult trc10Changes field
    org.tron.core.execution.spi.ExecutionProgramResult programResult =
        new org.tron.core.execution.spi.ExecutionProgramResult();

    assertNotNull("Trc10Changes should be initialized", programResult.getTrc10Changes());
    assertEquals("Trc10Changes should be empty by default", 0, programResult.getTrc10Changes().size());

    // Test setter
    java.util.List<org.tron.core.execution.spi.ExecutionSPI.Trc10LedgerChange> changes =
        new java.util.ArrayList<>();
    programResult.setTrc10Changes(changes);

    assertNotNull("Trc10Changes should not be null after setter", programResult.getTrc10Changes());
  }
}

package org.tron.core.execution.spi;

import org.junit.After;
import org.junit.Assert;
import org.junit.Test;

/**
 * Test class for ExecutionSpiFactory.
 */
public class ExecutionSpiFactoryTest {

  @After
  public void cleanup() {
    // Clear system properties after each test
    System.clearProperty("execution.mode");
    System.clearProperty("execution.remote.host");
    System.clearProperty("execution.remote.port");
  }

  @Test
  public void testDefaultExecutionMode() {
    ExecutionMode mode = ExecutionSpiFactory.determineExecutionMode();
    Assert.assertEquals(ExecutionMode.EMBEDDED, mode);
  }

  @Test
  public void testExecutionModeFromSystemProperty() {
    System.setProperty("execution.mode", "REMOTE");
    ExecutionMode mode = ExecutionSpiFactory.determineExecutionMode();
    Assert.assertEquals(ExecutionMode.REMOTE, mode);
  }

  @Test
  public void testExecutionModeFromSystemPropertyCaseInsensitive() {
    System.setProperty("execution.mode", "shadow");
    ExecutionMode mode = ExecutionSpiFactory.determineExecutionMode();
    Assert.assertEquals(ExecutionMode.SHADOW, mode);
  }

  @Test
  public void testInvalidExecutionMode() {
    System.setProperty("execution.mode", "INVALID");
    try {
      ExecutionSpiFactory.determineExecutionMode();
      Assert.fail("Should have thrown IllegalArgumentException");
    } catch (IllegalArgumentException e) {
      Assert.assertTrue(e.getMessage().contains("Invalid execution mode"));
    }
  }

  @Test
  public void testCreateEmbeddedExecution() {
    System.setProperty("execution.mode", "EMBEDDED");
    ExecutionSPI execution = ExecutionSpiFactory.createExecution();
    Assert.assertNotNull(execution);
    Assert.assertTrue(execution instanceof EmbeddedExecutionSPI);
  }

  @Test
  public void testCreateRemoteExecution() {
    System.setProperty("execution.mode", "REMOTE");
    ExecutionSPI execution = ExecutionSpiFactory.createExecution();
    Assert.assertNotNull(execution);
    Assert.assertTrue(execution instanceof RemoteExecutionSPI);
  }

  @Test
  public void testCreateShadowExecution() {
    System.setProperty("execution.mode", "SHADOW");
    ExecutionSPI execution = ExecutionSpiFactory.createExecution();
    Assert.assertNotNull(execution);
    Assert.assertTrue(execution instanceof ShadowExecutionSPI);
  }

  @Test
  public void testRemoteHostConfiguration() {
    System.setProperty("execution.remote.host", "test-host");
    String host = ExecutionSpiFactory.getRemoteHost();
    Assert.assertEquals("test-host", host);
  }

  @Test
  public void testRemotePortConfiguration() {
    System.setProperty("execution.remote.port", "9999");
    int port = ExecutionSpiFactory.getRemotePort();
    Assert.assertEquals(9999, port);
  }

  @Test
  public void testDefaultRemoteConfiguration() {
    String host = ExecutionSpiFactory.getRemoteHost();
    int port = ExecutionSpiFactory.getRemotePort();
    Assert.assertEquals("127.0.0.1", host);
    Assert.assertEquals(50012, port);
  }

  @Test
  public void testConfigurationInfo() {
    System.setProperty("execution.mode", "SHADOW");
    System.setProperty("execution.remote.host", "test-host");
    System.setProperty("execution.remote.port", "8888");

    String info = ExecutionSpiFactory.getConfigurationInfo();
    Assert.assertNotNull(info);
    Assert.assertTrue(info.contains("shadow"));
    Assert.assertTrue(info.contains("test-host"));
    Assert.assertTrue(info.contains("8888"));
  }

  @Test
  public void testExecutionModeFromString() {
    Assert.assertEquals(ExecutionMode.EMBEDDED, ExecutionMode.fromString("EMBEDDED"));
    Assert.assertEquals(ExecutionMode.REMOTE, ExecutionMode.fromString("remote"));
    Assert.assertEquals(ExecutionMode.SHADOW, ExecutionMode.fromString("Shadow"));
    Assert.assertEquals(ExecutionMode.EMBEDDED, ExecutionMode.fromString(null));
    Assert.assertEquals(ExecutionMode.EMBEDDED, ExecutionMode.fromString(""));
  }

  @Test
  public void testExecutionModeToString() {
    Assert.assertEquals("embedded", ExecutionMode.EMBEDDED.toString());
    Assert.assertEquals("remote", ExecutionMode.REMOTE.toString());
    Assert.assertEquals("shadow", ExecutionMode.SHADOW.toString());
  }

  @Test
  public void testCreateExecutionWithSpecificMode() {
    ExecutionSPI embedded = ExecutionSpiFactory.createExecution(ExecutionMode.EMBEDDED);
    Assert.assertTrue(embedded instanceof EmbeddedExecutionSPI);

    ExecutionSPI remote = ExecutionSpiFactory.createExecution(ExecutionMode.REMOTE);
    Assert.assertTrue(remote instanceof RemoteExecutionSPI);

    ExecutionSPI shadow = ExecutionSpiFactory.createExecution(ExecutionMode.SHADOW);
    Assert.assertTrue(shadow instanceof ShadowExecutionSPI);
  }

  @Test
  public void testCreateExecutionWithNullMode() {
    try {
      ExecutionSpiFactory.createExecution(null);
      Assert.fail("Should have thrown IllegalArgumentException");
    } catch (IllegalArgumentException e) {
      Assert.assertTrue(e.getMessage().contains("cannot be null"));
    }
  }
}

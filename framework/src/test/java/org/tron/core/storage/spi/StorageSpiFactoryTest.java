package org.tron.core.storage.spi;

import org.junit.After;
import org.junit.Assert;
import org.junit.Before;
import org.junit.Test;
import org.tron.common.parameter.CommonParameter;
import org.tron.core.config.args.Storage;

/** Unit tests for StorageSpiFactory configuration and implementation selection. */
public class StorageSpiFactoryTest {

  private String originalSystemProperty;
  private String originalEnvMode;
  private Storage originalStorage;

  @Before
  public void setUp() {
    // Save original values
    originalSystemProperty = System.getProperty("storage.mode");

    // Save original storage configuration from CommonParameter
    CommonParameter parameter = CommonParameter.getInstance();
    originalStorage = parameter.storage;

    // Set a clean storage configuration to avoid interference from previous tests
    Storage cleanStorage = new Storage();
    cleanStorage.setStorageMode(null); // Ensure no storage mode is set
    parameter.storage = cleanStorage;

    // Note: We can't actually modify environment variables in tests,
    // so we'll focus on system property testing
  }

  @After
  public void tearDown() {
    // Always clear the system property first to ensure clean state
    System.clearProperty("storage.mode");

    // Then restore original value if there was one
    if (originalSystemProperty != null) {
      System.setProperty("storage.mode", originalSystemProperty);
    }

    // Restore original storage configuration
    CommonParameter.getInstance().storage = originalStorage;

    // Clear other test properties
    System.clearProperty("storage.remote.host");
    System.clearProperty("storage.remote.port");
    System.clearProperty("storage.embedded.basePath");
  }

  @Test
  public void testStorageModeFromString() {
    // Test valid modes
    Assert.assertEquals(StorageMode.EMBEDDED, StorageMode.fromString("embedded"));
    Assert.assertEquals(StorageMode.EMBEDDED, StorageMode.fromString("EMBEDDED"));
    Assert.assertEquals(StorageMode.EMBEDDED, StorageMode.fromString("  embedded  "));

    Assert.assertEquals(StorageMode.REMOTE, StorageMode.fromString("remote"));
    Assert.assertEquals(StorageMode.REMOTE, StorageMode.fromString("REMOTE"));
    Assert.assertEquals(StorageMode.REMOTE, StorageMode.fromString("  remote  "));

    // Test null/empty defaults to default
    Assert.assertEquals(StorageMode.getDefault(), StorageMode.fromString(null));
    Assert.assertEquals(StorageMode.getDefault(), StorageMode.fromString(""));
    Assert.assertEquals(StorageMode.getDefault(), StorageMode.fromString("   "));

    // Test invalid mode
    try {
      StorageMode.fromString("invalid");
      Assert.fail("Should have thrown IllegalArgumentException");
    } catch (IllegalArgumentException e) {
      Assert.assertTrue(e.getMessage().contains("Invalid storage mode"));
    }
  }

  @Test
  public void testDetermineStorageModeDefault() {
    // Clear any existing system property
    System.clearProperty("storage.mode");

    StorageMode mode = StorageSpiFactory.determineStorageMode();
    Assert.assertEquals(StorageMode.getDefault(), mode);
  }

  @Test
  public void testDetermineStorageModeFromSystemProperty() {
    // Test system property takes precedence
    System.setProperty("storage.mode", "embedded");
    StorageMode mode = StorageSpiFactory.determineStorageMode();
    Assert.assertEquals(StorageMode.EMBEDDED, mode);

    System.setProperty("storage.mode", "remote");
    mode = StorageSpiFactory.determineStorageMode();
    Assert.assertEquals(StorageMode.REMOTE, mode);
  }

  @Test
  public void testCreateStorageEmbedded() {
    System.setProperty("storage.mode", "embedded");
    System.setProperty("storage.embedded.basePath", "test-data/embedded");

    StorageSPI storage = StorageSpiFactory.createStorage();
    Assert.assertNotNull(storage);
    Assert.assertTrue(storage instanceof EmbeddedStorageSPI);

    // Clean up
    if (storage instanceof EmbeddedStorageSPI) {
      ((EmbeddedStorageSPI) storage).close();
    }
  }

  @Test
  public void testCreateStorageRemote() {
    System.setProperty("storage.mode", "remote");
    System.setProperty("storage.remote.host", "test-host");
    System.setProperty("storage.remote.port", "9999");

    StorageSPI storage = StorageSpiFactory.createStorage();
    Assert.assertNotNull(storage);
    Assert.assertTrue(storage instanceof RemoteStorageSPI);

    // Clean up
    if (storage instanceof RemoteStorageSPI) {
      ((RemoteStorageSPI) storage).close();
    }
  }

  @Test
  public void testConfigurationInfo() {
    System.setProperty("storage.mode", "embedded");
    System.setProperty("storage.embedded.basePath", "test-data");

    String info = StorageSpiFactory.getConfigurationInfo();
    Assert.assertNotNull(info);
    Assert.assertTrue(info.contains("Mode: embedded"));
    Assert.assertTrue(info.contains("Base Path: test-data"));

    System.setProperty("storage.mode", "remote");
    System.setProperty("storage.remote.host", "test-host");
    System.setProperty("storage.remote.port", "8888");

    info = StorageSpiFactory.getConfigurationInfo();
    Assert.assertTrue(info.contains("Mode: remote"));
    Assert.assertTrue(info.contains("remote Host: test-host"));
    Assert.assertTrue(info.contains("remote Port: 8888"));
  }

  @Test
  public void testConfigFileSupport() {
    // Test config file methods with explicit Config object
    com.typesafe.config.Config testConfig =
        com.typesafe.config.ConfigFactory.parseString(
            "storage.mode = \"remote\"\n"
                + "storage.remote.host = \"config-host\"\n"
                + "storage.remote.port = 9999\n"
                + "storage.embedded.basePath = \"config-path\"");

    // Test storage mode from config
    StorageMode mode = StorageSpiFactory.determineStorageMode(testConfig);
    Assert.assertEquals(StorageMode.REMOTE, mode);

    // Test gRPC host from config
    String host = StorageSpiFactory.getRemoteHost(testConfig);
    Assert.assertEquals("config-host", host);

    // Test gRPC port from config
    int port = StorageSpiFactory.getRemotePort(testConfig);
    Assert.assertEquals(9999, port);

    // Test embedded base path from config
    String basePath = StorageSpiFactory.getEmbeddedBasePath(testConfig);
    Assert.assertEquals("config-path", basePath);
  }

  @Test
  public void testConfigFilePrecedence() {
    // Test that system properties take precedence over config file
    com.typesafe.config.Config testConfig =
        com.typesafe.config.ConfigFactory.parseString(
            "storage.mode = \"embedded\"\n"
                + "storage.remote.host = \"config-host\"\n"
                + "storage.remote.port = 8888");

    // Set system properties that should override config file
    System.setProperty("storage.mode", "remote");
    System.setProperty("storage.remote.host", "system-host");
    System.setProperty("storage.remote.port", "7777");

    // Test that system properties take precedence
    StorageMode mode = StorageSpiFactory.determineStorageMode(testConfig);
    Assert.assertEquals(StorageMode.REMOTE, mode);

    String host = StorageSpiFactory.getRemoteHost(testConfig);
    Assert.assertEquals("system-host", host);

    int port = StorageSpiFactory.getRemotePort(testConfig);
    Assert.assertEquals(7777, port);
  }

  @Test
  public void testConfigFileDefaults() {
    // Test with empty config file to ensure defaults are used
    com.typesafe.config.Config emptyConfig = com.typesafe.config.ConfigFactory.parseString("");

    // Clear any system properties
    System.clearProperty("storage.mode");
    System.clearProperty("storage.remote.host");
    System.clearProperty("storage.remote.port");
    System.clearProperty("storage.embedded.basePath");

    // Test defaults are used when config is empty
    StorageMode mode = StorageSpiFactory.determineStorageMode(emptyConfig);
    Assert.assertEquals(StorageMode.getDefault(), mode);

    String host = StorageSpiFactory.getRemoteHost(emptyConfig);
    Assert.assertEquals("localhost", host);

    int port = StorageSpiFactory.getRemotePort(emptyConfig);
    Assert.assertEquals(50011, port);

    String basePath = StorageSpiFactory.getEmbeddedBasePath(emptyConfig);
    Assert.assertEquals("data/rocksdb-embedded", basePath);
  }

  @Test
  public void testInvalidPortConfiguration() {
    System.setProperty("storage.mode", "remote");
    System.setProperty("storage.remote.port", "invalid-port");

    // Should still create storage with default port
    StorageSPI storage = StorageSpiFactory.createStorage();
    Assert.assertNotNull(storage);
    Assert.assertTrue(storage instanceof RemoteStorageSPI);

    // Clean up
    if (storage instanceof RemoteStorageSPI) {
      ((RemoteStorageSPI) storage).close();
    }
  }

  @Test
  public void testStorageModeToString() {
    Assert.assertEquals("embedded", StorageMode.EMBEDDED.toString());
    Assert.assertEquals("remote", StorageMode.REMOTE.toString());
  }

  @Test
  public void testDefaultStorageMode() {
    // Verify default is EMBEDDED (for backward compatibility and conservative defaults)
    Assert.assertEquals(StorageMode.EMBEDDED, StorageMode.getDefault());
  }
}

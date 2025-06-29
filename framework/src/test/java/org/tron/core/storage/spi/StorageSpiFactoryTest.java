package org.tron.core.storage.spi;

import org.junit.Test;
import org.junit.After;
import org.junit.Assert;
import org.junit.Before;

/**
 * Unit tests for StorageSpiFactory configuration and implementation selection.
 */
public class StorageSpiFactoryTest {
    
    private String originalSystemProperty;
    private String originalEnvMode;
    
    @Before
    public void setUp() {
        // Save original values
        originalSystemProperty = System.getProperty("storage.mode");
        // Note: We can't actually modify environment variables in tests,
        // so we'll focus on system property testing
    }
    
    @After
    public void tearDown() {
        // Restore original values
        if (originalSystemProperty != null) {
            System.setProperty("storage.mode", originalSystemProperty);
        } else {
            System.clearProperty("storage.mode");
        }
        
        // Clear other test properties
        System.clearProperty("storage.grpc.host");
        System.clearProperty("storage.grpc.port");
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
        System.setProperty("storage.grpc.host", "test-host");
        System.setProperty("storage.grpc.port", "9999");
        
        StorageSPI storage = StorageSpiFactory.createStorage();
        Assert.assertNotNull(storage);
        Assert.assertTrue(storage instanceof GrpcStorageSPI);
        
        // Clean up
        if (storage instanceof GrpcStorageSPI) {
            ((GrpcStorageSPI) storage).close();
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
        System.setProperty("storage.grpc.host", "test-host");
        System.setProperty("storage.grpc.port", "8888");
        
        info = StorageSpiFactory.getConfigurationInfo();
        Assert.assertTrue(info.contains("Mode: remote"));
        Assert.assertTrue(info.contains("gRPC Host: test-host"));
        Assert.assertTrue(info.contains("gRPC Port: 8888"));
    }
    
    @Test
    public void testInvalidPortConfiguration() {
        System.setProperty("storage.mode", "remote");
        System.setProperty("storage.grpc.port", "invalid-port");
        
        // Should still create storage with default port
        StorageSPI storage = StorageSpiFactory.createStorage();
        Assert.assertNotNull(storage);
        Assert.assertTrue(storage instanceof GrpcStorageSPI);
        
        // Clean up
        if (storage instanceof GrpcStorageSPI) {
            ((GrpcStorageSPI) storage).close();
        }
    }
    
    @Test
    public void testStorageModeToString() {
        Assert.assertEquals("embedded", StorageMode.EMBEDDED.toString());
        Assert.assertEquals("remote", StorageMode.REMOTE.toString());
    }
    
    @Test
    public void testDefaultStorageMode() {
        // Verify default is REMOTE (as per our design decision)
        Assert.assertEquals(StorageMode.REMOTE, StorageMode.getDefault());
    }
} 
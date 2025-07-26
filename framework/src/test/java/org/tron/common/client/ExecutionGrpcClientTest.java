package org.tron.common.client;

import org.junit.After;
import org.junit.Assert;
import org.junit.Before;
import org.junit.Test;
import tron.backend.BackendOuterClass.*;

/**
 * Test class for ExecutionGrpcClient.
 * Note: These tests verify client creation and basic functionality.
 * Full integration tests require a running Rust backend service.
 */
public class ExecutionGrpcClientTest {

  private ExecutionGrpcClient client;

  @Before
  public void setUp() {
    // Create client with default test configuration
    client = new ExecutionGrpcClient("localhost", 50011);
  }

  @After
  public void tearDown() {
    if (client != null && !client.isShutdown()) {
      client.shutdown();
    }
  }

  @Test
  public void testClientCreation() {
    Assert.assertNotNull(client);
    Assert.assertFalse(client.isShutdown());
    Assert.assertFalse(client.isTerminated());
  }

  @Test
  public void testClientCreationWithTarget() {
    ExecutionGrpcClient targetClient = new ExecutionGrpcClient("localhost:50011");
    Assert.assertNotNull(targetClient);
    Assert.assertFalse(targetClient.isShutdown());
    targetClient.shutdown();
  }

  @Test
  public void testInvalidHostThrowsException() {
    try {
      new ExecutionGrpcClient(null, 50011);
      Assert.fail("Should have thrown IllegalArgumentException");
    } catch (IllegalArgumentException e) {
      Assert.assertTrue(e.getMessage().contains("Host cannot be null"));
    }

    try {
      new ExecutionGrpcClient("", 50011);
      Assert.fail("Should have thrown IllegalArgumentException");
    } catch (IllegalArgumentException e) {
      Assert.assertTrue(e.getMessage().contains("Host cannot be null"));
    }
  }

  @Test
  public void testInvalidPortThrowsException() {
    try {
      new ExecutionGrpcClient("localhost", 0);
      Assert.fail("Should have thrown IllegalArgumentException");
    } catch (IllegalArgumentException e) {
      Assert.assertTrue(e.getMessage().contains("Port must be between"));
    }

    try {
      new ExecutionGrpcClient("localhost", 70000);
      Assert.fail("Should have thrown IllegalArgumentException");
    } catch (IllegalArgumentException e) {
      Assert.assertTrue(e.getMessage().contains("Port must be between"));
    }
  }

  @Test
  public void testInvalidTargetThrowsException() {
    try {
      new ExecutionGrpcClient(null);
      Assert.fail("Should have thrown IllegalArgumentException");
    } catch (IllegalArgumentException e) {
      Assert.assertTrue(e.getMessage().contains("Target cannot be null"));
    }

    try {
      new ExecutionGrpcClient("");
      Assert.fail("Should have thrown IllegalArgumentException");
    } catch (IllegalArgumentException e) {
      Assert.assertTrue(e.getMessage().contains("Target cannot be null"));
    }
  }

  @Test
  public void testShutdown() {
    Assert.assertFalse(client.isShutdown());
    client.shutdown();
    Assert.assertTrue(client.isShutdown());
  }

  @Test
  public void testHealthCheckRequestCreation() {
    // Test that we can create health check requests
    HealthRequest request = HealthRequest.newBuilder().build();
    Assert.assertNotNull(request);
  }

  @Test
  public void testMetadataRequestCreation() {
    // Test that we can create metadata requests
    MetadataRequest request = MetadataRequest.newBuilder().build();
    Assert.assertNotNull(request);
  }

  @Test
  public void testExecuteTransactionRequestCreation() {
    // Test that we can create execute transaction requests
    TronTransaction transaction = TronTransaction.newBuilder()
        .setFrom(com.google.protobuf.ByteString.copyFrom(new byte[20]))
        .setTo(com.google.protobuf.ByteString.copyFrom(new byte[20]))
        .setValue(com.google.protobuf.ByteString.copyFrom(new byte[32]))
        .setData(com.google.protobuf.ByteString.copyFrom(new byte[0]))
        .setEnergyLimit(1000000)
        .setEnergyPrice(1)
        .setNonce(0)
        .build();

    ExecutionContext context = ExecutionContext.newBuilder()
        .setBlockNumber(1)
        .setBlockTimestamp(System.currentTimeMillis())
        .setBlockHash(com.google.protobuf.ByteString.copyFrom(new byte[32]))
        .setCoinbase(com.google.protobuf.ByteString.copyFrom(new byte[20]))
        .setEnergyLimit(1000000)
        .setEnergyPrice(1)
        .build();

    ExecuteTransactionRequest request = ExecuteTransactionRequest.newBuilder()
        .setDatabase("test")
        .setTransaction(transaction)
        .setContext(context)
        .build();

    Assert.assertNotNull(request);
    Assert.assertEquals("test", request.getDatabase());
    Assert.assertEquals(1000000, request.getTransaction().getEnergyLimit());
  }

  @Test
  public void testCallContractRequestCreation() {
    // Test that we can create call contract requests
    CallContractRequest request = CallContractRequest.newBuilder()
        .setDatabase("test")
        .setFrom(com.google.protobuf.ByteString.copyFrom(new byte[20]))
        .setTo(com.google.protobuf.ByteString.copyFrom(new byte[20]))
        .setData(com.google.protobuf.ByteString.copyFrom(new byte[0]))
        .setContext(ExecutionContext.newBuilder()
            .setBlockNumber(1)
            .setBlockTimestamp(System.currentTimeMillis())
            .build())
        .build();

    Assert.assertNotNull(request);
    Assert.assertEquals("test", request.getDatabase());
  }

  @Test
  public void testEstimateEnergyRequestCreation() {
    // Test that we can create estimate energy requests
    TronTransaction transaction = TronTransaction.newBuilder()
        .setFrom(com.google.protobuf.ByteString.copyFrom(new byte[20]))
        .setTo(com.google.protobuf.ByteString.copyFrom(new byte[20]))
        .setEnergyLimit(1000000)
        .build();

    EstimateEnergyRequest request = EstimateEnergyRequest.newBuilder()
        .setDatabase("test")
        .setTransaction(transaction)
        .setContext(ExecutionContext.newBuilder()
            .setBlockNumber(1)
            .build())
        .build();

    Assert.assertNotNull(request);
    Assert.assertEquals("test", request.getDatabase());
  }

  // Note: Integration tests that actually call the gRPC methods would require
  // a running Rust backend service. These would be added in a separate
  // integration test suite.
}

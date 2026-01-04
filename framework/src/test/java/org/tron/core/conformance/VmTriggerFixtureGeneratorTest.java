package org.tron.core.conformance;

import com.google.protobuf.ByteString;
import java.io.File;
import java.io.FileOutputStream;
import java.util.Iterator;
import java.util.Map;
import java.util.SortedMap;
import java.util.TreeMap;
import org.bouncycastle.util.encoders.Hex;
import org.junit.Assert;
import org.junit.Before;
import org.junit.Test;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.runtime.Runtime;
import org.tron.common.runtime.TVMTestResult;
import org.tron.common.runtime.TvmTestUtils;
import org.tron.common.runtime.vm.VMTestBase;
import org.tron.common.utils.WalletUtil;
import org.tron.core.capsule.BlockCapsule;
import org.tron.protos.Protocol;
import org.tron.protos.Protocol.Transaction;
import org.tron.protos.contract.SmartContractOuterClass.TriggerSmartContract;
import tron.backend.BackendOuterClass.ContractType;
import tron.backend.BackendOuterClass.ExecuteTransactionRequest;
import tron.backend.BackendOuterClass.ExecutionContext;
import tron.backend.BackendOuterClass.TronTransaction;
import tron.backend.BackendOuterClass.TxKind;

/**
 * Generates conformance test fixtures for TriggerSmartContract (31) using VMTestBase.
 *
 * <p>This test extends VMTestBase to get proper TVM test infrastructure including:
 * - Repository-based state management (rootRepository)
 * - Manager for transaction execution
 * - Proper test isolation
 *
 * <p>Run with: ./gradlew :framework:test --tests "VmTriggerFixtureGeneratorTest"
 *              -Dconformance.output=../conformance/fixtures
 */
public class VmTriggerFixtureGeneratorTest extends VMTestBase {

  private static final Logger log = LoggerFactory.getLogger(VmTriggerFixtureGeneratorTest.class);
  private static final long DEFAULT_FEE_LIMIT = 100_000_000L; // 100 TRX

  // Deterministic timestamps for fixture generation
  private static final long FIXED_BLOCK_TIMESTAMP = 1700000000000L; // 2023-11-14 22:13:20 UTC
  private static final long FIXED_BLOCK_NUMBER = 1L;

  // Known max fee limit for validation fixtures
  private static final long KNOWN_MAX_FEE_LIMIT = 1_000_000_000L; // 1000 TRX

  private File outputDir;

  // StorageDemo contract
  private static final String STORAGE_ABI = "[{\"constant\":true,\"inputs\":[{\"name\":\"\","
      + "\"type\":\"uint256\"}],\"name\":\"int2str\",\"outputs\":[{\"name\":\"\",\"type\":\"string\"}],"
      + "\"payable\":false,\"stateMutability\":\"view\",\"type\":\"function\"},{\"constant\":false,"
      + "\"inputs\":[{\"name\":\"i\",\"type\":\"uint256\"}],\"name\":\"testDelete\",\"outputs\":[],"
      + "\"payable\":false,\"stateMutability\":\"nonpayable\",\"type\":\"function\"},{\"constant\":false,"
      + "\"inputs\":[{\"name\":\"i\",\"type\":\"uint256\"},{\"name\":\"s\",\"type\":\"string\"}],"
      + "\"name\":\"testPut\",\"outputs\":[],\"payable\":false,\"stateMutability\":\"nonpayable\","
      + "\"type\":\"function\"}]";

  private static final String STORAGE_CODE = "608060405234801561001057600080fd5b50610341806100206000"
      + "396000f30060806040526004361061005657"
      + "63ffffffff7c01000000000000000000000000000000000000000000000000000000006000350416"
      + "6313d821f4811461005b57806330099fa9146100e8578063c38e31cc14610102575b600080fd5b3480"
      + "1561006757600080fd5b50610073600435610160565b6040805160208082528351818301528351919283"
      + "929083019185019080838360005b838110156100ad578181015183820152602001610095565b5050505090"
      + "5090810190601f1680156100da5780820380516001836020036101000a031916815260200191505b509250"
      + "505060405180910390f35b3480156100f457600080fd5b506101006004356101fa565b005b348015610"
      + "10e57600080fd5b506040805160206004602480358281013560"
      + "1f8101859004850286018501909652858552610100958335953695604494919390910191908190840183"
      + "8280828437509497506102149650505050505050565b6000602081815291815260409081902080548251"
      + "60026001831615610100026000190190921691909104601f810185900485028201850190935282815292"
      + "9091908301828280156101f25780601f106101c7576101008083540402835291602001916101f2565b82"
      + "0191906000526020600020905b8154815290600101906020018083116101d557829003601f168201915b"
      + "505050505081565b600081815260208190526040812061021191610236565b50565b60008281526020"
      + "818152604090912082516102319284019061027a565b505050565b5080546001816001161561010002031"
      + "6600290046000825580601f1061025c5750610211565b601f01602090049060005260206000209081019"
      + "061021191906102f8565b828054600181600116156101000203166002900490600052602060002090601"
      + "f016020900481019282601f106102bb57805160ff19168380011785556102e8565b82800160010185558"
      + "21561"
      + "02e8579182015b828111156102e85782518255916020019190600101906102cd565b506102f492915061"
      + "02f8565b5090565b61031291905b808211156102f457600081556001016102fe565b905600a165627a7a"
      + "72305820c98643943ea978505f9cca68bdf61681462daeee9f71a6aa4414609e48dbb46b0029";

  @Before
  public void setup() {
    String outputPath = System.getProperty("conformance.output", "../conformance/fixtures");
    outputDir = new File(outputPath);
    log.info("VM Trigger Fixture output directory: {}", outputDir.getAbsolutePath());
  }

  // ==========================================================================
  // TriggerSmartContract (31) Fixtures
  // ==========================================================================

  @Test
  public void generateTriggerSmartContract_happyPath() throws Exception {
    log.info("Generating TriggerSmartContract happy path fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract using rootRepository (from VMTestBase)
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    log.info("Deployed contract at: {}", Hex.toHexString(contractAddress));

    // Capture pre-trigger state
    File fixtureDir = new File(outputDir, "trigger_smart_contract/happy_path");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Trigger contract - call testPut(1, "hello")
    String params = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000005"
        + "68656c6c6f000000000000000000000000000000000000000000000000000000";
    byte[] triggerData = TvmTestUtils.parseAbi("testPut(uint256,string)", params);

    TVMTestResult result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, triggerData, 0, DEFAULT_FEE_LIMIT, manager, null);

    Assert.assertNull("Trigger should succeed", result.getRuntime().getRuntimeError());
    log.info("Trigger executed successfully, energy used: {}", result.getReceipt().getEnergyUsageTotal());

    // Build request proto
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-trigger state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt
    if (result.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        result.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("happy_path")
        .caseCategory("happy")
        .description("Trigger contract to store a value via testPut(1, 'hello')")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    metadata.setExpectedStatus("SUCCESS");
    metadata.toFile(new File(fixtureDir, "metadata.json"));

    log.info("Generated TriggerSmartContract happy_path fixture");
  }

  @Test
  public void generateTriggerSmartContract_storageWrite() throws Exception {
    log.info("Generating TriggerSmartContract storage write fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // First write to initialize storage
    String params1 = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000003"
        + "6162630000000000000000000000000000000000000000000000000000000000"; // "abc"
    byte[] triggerData1 = TvmTestUtils.parseAbi("testPut(uint256,string)", params1);
    TVMTestResult result1 = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, triggerData1, 0, DEFAULT_FEE_LIMIT, manager, null);
    Assert.assertNull("First write should succeed", result1.getRuntime().getRuntimeError());

    // Capture pre-state for second write
    File fixtureDir = new File(outputDir, "trigger_smart_contract/storage_overwrite");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Overwrite storage with different value
    String params2 = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000003"
        + "7879790000000000000000000000000000000000000000000000000000000000"; // "xyz"
    byte[] triggerData2 = TvmTestUtils.parseAbi("testPut(uint256,string)", params2);
    TVMTestResult result2 = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, triggerData2, 0, DEFAULT_FEE_LIMIT, manager, null);
    Assert.assertNull("Storage overwrite should succeed", result2.getRuntime().getRuntimeError());

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData2))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt
    if (result2.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        result2.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("storage_overwrite")
        .caseCategory("happy")
        .description("Overwrite existing storage slot with new value")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    metadata.setExpectedStatus("SUCCESS");
    metadata.toFile(new File(fixtureDir, "metadata.json"));

    log.info("Generated TriggerSmartContract storage_overwrite fixture");
  }

  @Test
  public void generateTriggerSmartContract_viewFunction() throws Exception {
    log.info("Generating TriggerSmartContract view function fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // First write a value
    String putParams = "0000000000000000000000000000000000000000000000000000000000000042"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000004"
        + "7465737400000000000000000000000000000000000000000000000000000000"; // "test" at key 66
    byte[] putData = TvmTestUtils.parseAbi("testPut(uint256,string)", putParams);
    TVMTestResult putResult = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, putData, 0, DEFAULT_FEE_LIMIT, manager, null);
    Assert.assertNull("Put should succeed", putResult.getRuntime().getRuntimeError());

    // Capture pre-state for view call
    File fixtureDir = new File(outputDir, "trigger_smart_contract/view_function");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Call view function int2str(66)
    String viewParams = "0000000000000000000000000000000000000000000000000000000000000042";
    byte[] viewData = TvmTestUtils.parseAbi("int2str(uint256)", viewParams);
    TVMTestResult viewResult = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, viewData, 0, DEFAULT_FEE_LIMIT, manager, null);
    Assert.assertNull("View should succeed", viewResult.getRuntime().getRuntimeError());

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(viewData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state (should be same as pre-state for view)
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt
    if (viewResult.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        viewResult.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("view_function")
        .caseCategory("happy")
        .description("Call view function int2str(66) to read stored value")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    metadata.setExpectedStatus("SUCCESS");
    metadata.toFile(new File(fixtureDir, "metadata.json"));

    log.info("Generated TriggerSmartContract view_function fixture");
  }

  @Test
  public void generateTriggerSmartContract_deleteStorage() throws Exception {
    log.info("Generating TriggerSmartContract delete storage fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // First write a value
    String putParams = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000005"
        + "68656c6c6f000000000000000000000000000000000000000000000000000000"; // "hello" at key 1
    byte[] putData = TvmTestUtils.parseAbi("testPut(uint256,string)", putParams);
    TVMTestResult putResult = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, putData, 0, DEFAULT_FEE_LIMIT, manager, null);
    Assert.assertNull("Put should succeed", putResult.getRuntime().getRuntimeError());

    // Capture pre-state for delete
    File fixtureDir = new File(outputDir, "trigger_smart_contract/delete_storage");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Delete storage at key 1
    String deleteParams = "0000000000000000000000000000000000000000000000000000000000000001";
    byte[] deleteData = TvmTestUtils.parseAbi("testDelete(uint256)", deleteParams);
    TVMTestResult deleteResult = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, deleteData, 0, DEFAULT_FEE_LIMIT, manager, null);
    Assert.assertNull("Delete should succeed", deleteResult.getRuntime().getRuntimeError());

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(deleteData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt
    if (deleteResult.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        deleteResult.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("delete_storage")
        .caseCategory("happy")
        .description("Delete storage slot using testDelete(1)")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    metadata.setExpectedStatus("SUCCESS");
    metadata.toFile(new File(fixtureDir, "metadata.json"));

    log.info("Generated TriggerSmartContract delete_storage fixture");
  }

  @Test
  public void generateTriggerSmartContract_nonexistentContract() throws Exception {
    log.info("Generating TriggerSmartContract nonexistent contract fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Use a fake contract address that doesn't exist
    byte[] fakeContractAddress = Hex.decode("410000000000000000000000000000000000000001");

    // Capture pre-state
    File fixtureDir = new File(outputDir, "trigger_smart_contract/edge_nonexistent_contract");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Try to call a function on non-existent contract
    String params = "0000000000000000000000000000000000000000000000000000000000000001";
    byte[] triggerData = TvmTestUtils.parseAbi("testDelete(uint256)", params);

    String errorMessage = null;
    TVMTestResult result = null;
    try {
      result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
          ownerBytes, fakeContractAddress, triggerData, 0, DEFAULT_FEE_LIMIT, manager, null);
      if (result.getRuntime().getRuntimeError() != null) {
        errorMessage = result.getRuntime().getRuntimeError();
      }
    } catch (Exception e) {
      errorMessage = e.getMessage();
    }

    log.info("Nonexistent contract trigger result: error={}", errorMessage);

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(fakeContractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt if available
    if (result != null && result.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        result.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("edge_nonexistent_contract")
        .caseCategory("edge")
        .description("Trigger a contract address that does not exist")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    if (errorMessage != null) {
      metadata.setExpectedStatus("REVERT");
      metadata.setExpectedErrorMessage(errorMessage);
    } else {
      metadata.setExpectedStatus("SUCCESS");
    }
    metadata.toFile(new File(fixtureDir, "metadata.json"));

    log.info("Generated TriggerSmartContract edge_nonexistent_contract fixture");
  }

  @Test
  public void generateTriggerSmartContract_outOfEnergy() throws Exception {
    log.info("Generating TriggerSmartContract out of energy fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract first
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    log.info("Deployed contract at: {}", Hex.toHexString(contractAddress));

    // Capture pre-state
    File fixtureDir = new File(outputDir, "trigger_smart_contract/edge_out_of_energy");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Trigger with very low fee limit (1 unit) - should run out of energy
    long veryLowFeeLimit = 1L;
    String params = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000005"
        + "68656c6c6f000000000000000000000000000000000000000000000000000000";
    byte[] triggerData = TvmTestUtils.parseAbi("testPut(uint256,string)", params);

    String errorMessage = null;
    TVMTestResult result = null;
    try {
      result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
          ownerBytes, contractAddress, triggerData, 0, veryLowFeeLimit, manager, null);
      if (result.getRuntime().getRuntimeError() != null) {
        errorMessage = result.getRuntime().getRuntimeError();
      }
    } catch (Exception e) {
      errorMessage = e.getMessage();
    }

    log.info("Out of energy trigger result: error={}", errorMessage);

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .build();

    // Use the low fee limit in the request
    ExecuteTransactionRequest request = buildTriggerRequestWithFeeLimit(
        triggerContract, null, veryLowFeeLimit);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt if available
    if (result != null && result.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        result.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("edge_out_of_energy")
        .caseCategory("edge")
        .description("Trigger contract with insufficient energy/fee limit")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    if (errorMessage != null) {
      metadata.setExpectedStatus("OUT_OF_ENERGY");
      metadata.setExpectedErrorMessage(errorMessage);
    } else {
      metadata.setExpectedStatus("SUCCESS");
    }
    metadata.toFile(new File(fixtureDir, "metadata.json"));

    log.info("Generated TriggerSmartContract edge_out_of_energy fixture");
  }

  // ==========================================================================
  // Phase 1: Validation Failure Fixtures
  // ==========================================================================

  @Test
  public void generateTriggerSmartContract_validateFailFeeLimitNegative() throws Exception {
    log.info("Generating TriggerSmartContract validate_fail_fee_limit_negative fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract first
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Set known max fee limit for stable error message
    manager.getDynamicPropertiesStore().saveMaxFeeLimit(KNOWN_MAX_FEE_LIMIT);

    // Capture pre-state
    File fixtureDir = new File(outputDir, "trigger_smart_contract/validate_fail_fee_limit_negative");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Trigger with negative feeLimit
    String params = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000005"
        + "68656c6c6f000000000000000000000000000000000000000000000000000000";
    byte[] triggerData = TvmTestUtils.parseAbi("testPut(uint256,string)", params);

    long negativeFeeLimit = -1L;
    String expectedError = "feeLimit must be >= 0 and <= " + KNOWN_MAX_FEE_LIMIT;
    String errorMessage = null;
    TVMTestResult result = null;

    // Note: The feeLimit validation happens inside VMActuator.call() after the contract
    // is validated to exist. With a negative feeLimit, the validation should fail.
    try {
      result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
          ownerBytes, contractAddress, triggerData, 0, negativeFeeLimit, manager, null);
      if (result.getRuntime().getRuntimeError() != null) {
        errorMessage = result.getRuntime().getRuntimeError();
      }
    } catch (org.tron.core.exception.ContractValidateException e) {
      errorMessage = e.getMessage();
      log.info("Got expected validation error: {}", errorMessage);
    } catch (Exception e) {
      errorMessage = e.getMessage();
      log.warn("Got exception: {}", e.getClass().getName());
    }

    log.info("Negative feeLimit result: error={}", errorMessage);

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequestWithFeeLimit(
        triggerContract, null, negativeFeeLimit);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state (should be unchanged)
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("validate_fail_fee_limit_negative")
        .caseCategory("validate_fail")
        .description("Trigger with negative feeLimit should fail validation")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError(expectedError)
        .build();

    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated TriggerSmartContract validate_fail_fee_limit_negative fixture");
  }

  @Test
  public void generateTriggerSmartContract_validateFailFeeLimitAboveMax() throws Exception {
    log.info("Generating TriggerSmartContract validate_fail_fee_limit_above_max fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract first
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Set known max fee limit
    manager.getDynamicPropertiesStore().saveMaxFeeLimit(KNOWN_MAX_FEE_LIMIT);

    // Capture pre-state
    File fixtureDir = new File(outputDir, "trigger_smart_contract/validate_fail_fee_limit_above_max");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Trigger with feeLimit > maxFeeLimit
    String params = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000005"
        + "68656c6c6f000000000000000000000000000000000000000000000000000000";
    byte[] triggerData = TvmTestUtils.parseAbi("testPut(uint256,string)", params);

    long aboveMaxFeeLimit = KNOWN_MAX_FEE_LIMIT + 1;
    String expectedError = "feeLimit must be >= 0 and <= " + KNOWN_MAX_FEE_LIMIT;
    String errorMessage = null;
    TVMTestResult result = null;

    try {
      result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
          ownerBytes, contractAddress, triggerData, 0, aboveMaxFeeLimit, manager, null);
      if (result.getRuntime().getRuntimeError() != null) {
        errorMessage = result.getRuntime().getRuntimeError();
      }
    } catch (org.tron.core.exception.ContractValidateException e) {
      errorMessage = e.getMessage();
      log.info("Got expected validation error: {}", errorMessage);
    } catch (Exception e) {
      errorMessage = e.getMessage();
    }

    log.info("Above-max feeLimit result: error={}", errorMessage);

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequestWithFeeLimit(
        triggerContract, null, aboveMaxFeeLimit);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("validate_fail_fee_limit_above_max")
        .caseCategory("validate_fail")
        .description("Trigger with feeLimit > maxFeeLimit should fail validation")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError(expectedError)
        .build();

    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated TriggerSmartContract validate_fail_fee_limit_above_max fixture");
  }

  @Test
  public void generateTriggerSmartContract_validateFailContractNotSmartContract() throws Exception {
    log.info("Generating TriggerSmartContract validate_fail_contract_not_smart_contract fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Use a valid address that exists as a normal account but has no contract
    // (the OWNER_ADDRESS is such an address - it's in AccountStore but not ContractStore)
    byte[] nonContractAddress = Hex.decode(OWNER_ADDRESS);

    // Capture pre-state
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/validate_fail_contract_not_smart_contract");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Try to trigger with a non-contract address
    String params = "0000000000000000000000000000000000000000000000000000000000000001";
    byte[] triggerData = TvmTestUtils.parseAbi("testDelete(uint256)", params);

    String expectedError = "No contract or not a smart contract";
    String errorMessage = null;
    TVMTestResult result = null;

    try {
      result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
          ownerBytes, nonContractAddress, triggerData, 0, DEFAULT_FEE_LIMIT, manager, null);
      if (result.getRuntime().getRuntimeError() != null) {
        errorMessage = result.getRuntime().getRuntimeError();
      }
    } catch (org.tron.core.exception.ContractValidateException e) {
      errorMessage = e.getMessage();
      log.info("Got expected validation error: {}", errorMessage);
    } catch (Exception e) {
      errorMessage = e.getMessage();
    }

    log.info("Non-contract trigger result: error={}", errorMessage);

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(nonContractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("validate_fail_contract_not_smart_contract")
        .caseCategory("validate_fail")
        .description("Trigger a valid address that exists but is not a smart contract")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError(expectedError)
        .build();

    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated TriggerSmartContract validate_fail_contract_not_smart_contract fixture");
  }

  @Test
  public void generateTriggerSmartContract_validateFailTokenValuePositiveTokenIdZero()
      throws Exception {
    log.info("Generating TriggerSmartContract validate_fail_token_value_positive_token_id_zero");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract first
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Enable TRC-10 transfer and multi-sign (required for checkTokenValueAndId)
    manager.getDynamicPropertiesStore().saveAllowTvmTransferTrc10(1);
    manager.getDynamicPropertiesStore().saveAllowMultiSign(1);

    // Capture pre-state
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/validate_fail_token_value_positive_token_id_zero");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Try to trigger with tokenValue > 0 and tokenId = 0
    String params = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000005"
        + "68656c6c6f000000000000000000000000000000000000000000000000000000";
    byte[] triggerData = TvmTestUtils.parseAbi("testPut(uint256,string)", params);

    long tokenValue = 100L;
    long tokenId = 0L;
    String expectedError = "invalid arguments with tokenValue = " + tokenValue + ", tokenId = 0";

    // Build trigger contract with token args
    // Note: The validation for tokenValue > 0 with tokenId = 0 happens in
    // VMActuator.checkTokenValueAndId() when the trigger is executed.
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .setCallTokenValue(tokenValue)
        .setTokenId(tokenId)
        .build();

    log.info("Token validation fixture: expected error = {}", expectedError);

    // Save request
    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("validate_fail_token_value_positive_token_id_zero")
        .caseCategory("validate_fail")
        .description("Trigger with callTokenValue > 0 and tokenId = 0 should fail")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError(expectedError)
        .build();

    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated validate_fail_token_value_positive_token_id_zero fixture");
  }

  @Test
  public void generateTriggerSmartContract_validateFailTokenIdTooSmall() throws Exception {
    log.info("Generating TriggerSmartContract validate_fail_token_id_too_small fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract first
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Enable TRC-10 transfer and multi-sign
    manager.getDynamicPropertiesStore().saveAllowTvmTransferTrc10(1);
    manager.getDynamicPropertiesStore().saveAllowMultiSign(1);

    // Capture pre-state
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/validate_fail_token_id_too_small");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Try with tokenId = 1000000 (MIN_TOKEN_ID, should fail)
    String params = "0000000000000000000000000000000000000000000000000000000000000001";
    byte[] triggerData = TvmTestUtils.parseAbi("testDelete(uint256)", params);

    long tokenValue = 100L;
    long tokenId = 1_000_000L; // MIN_TOKEN_ID
    String expectedError = "tokenId must be > 1000000";

    // Note: The validation for tokenId <= MIN_TOKEN_ID happens in
    // VMActuator.checkTokenValueAndId() when the trigger is executed.
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .setCallTokenValue(tokenValue)
        .setTokenId(tokenId)
        .build();

    log.info("Token ID validation fixture: expected error = {}", expectedError);

    // Save request
    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("validate_fail_token_id_too_small")
        .caseCategory("validate_fail")
        .description("Trigger with tokenId <= MIN_TOKEN_ID (1000000) should fail")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError(expectedError)
        .build();

    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated TriggerSmartContract validate_fail_token_id_too_small fixture");
  }

  // ==========================================================================
  // Phase 2: Runtime Parity Fixtures (REVERT cases)
  // ==========================================================================

  @Test
  public void generateTriggerSmartContract_edgeEmptyCalldataRevert() throws Exception {
    log.info("Generating TriggerSmartContract edge_empty_calldata_revert fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract first
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Capture pre-state
    File fixtureDir = new File(outputDir, "trigger_smart_contract/edge_empty_calldata_revert");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Trigger with empty calldata
    byte[] emptyData = new byte[0];
    String errorMessage = null;
    TVMTestResult result = null;

    try {
      result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
          ownerBytes, contractAddress, emptyData, 0, DEFAULT_FEE_LIMIT, manager, null);
      if (result.getRuntime().getRuntimeError() != null) {
        errorMessage = result.getRuntime().getRuntimeError();
      }
    } catch (Exception e) {
      errorMessage = e.getMessage();
    }

    log.info("Empty calldata trigger result: error={}", errorMessage);

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.EMPTY)
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt if available
    if (result != null && result.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        result.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata.Builder metadataBuilder = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("edge_empty_calldata_revert")
        .caseCategory("edge")
        .description("Trigger contract with empty calldata (no function selector)")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS);

    if (errorMessage != null) {
      metadataBuilder.expectedRevert(errorMessage);
    } else {
      metadataBuilder.expectedStatus("SUCCESS");
    }

    FixtureMetadata metadata = metadataBuilder.build();
    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated TriggerSmartContract edge_empty_calldata_revert fixture");
  }

  @Test
  public void generateTriggerSmartContract_edgeUnknownSelectorRevert() throws Exception {
    log.info("Generating TriggerSmartContract edge_unknown_selector_revert fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract first
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Capture pre-state
    File fixtureDir = new File(outputDir, "trigger_smart_contract/edge_unknown_selector_revert");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Trigger with unknown function selector (0xdeadbeef)
    byte[] unknownSelector = Hex.decode("deadbeef");
    String errorMessage = null;
    TVMTestResult result = null;

    try {
      result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
          ownerBytes, contractAddress, unknownSelector, 0, DEFAULT_FEE_LIMIT, manager, null);
      if (result.getRuntime().getRuntimeError() != null) {
        errorMessage = result.getRuntime().getRuntimeError();
      }
    } catch (Exception e) {
      errorMessage = e.getMessage();
    }

    log.info("Unknown selector trigger result: error={}", errorMessage);

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(unknownSelector))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt if available
    if (result != null && result.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        result.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata.Builder metadataBuilder = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("edge_unknown_selector_revert")
        .caseCategory("edge")
        .description("Trigger contract with unknown function selector (0xdeadbeef)")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS);

    if (errorMessage != null) {
      metadataBuilder.expectedRevert(errorMessage);
    } else {
      metadataBuilder.expectedStatus("SUCCESS");
    }

    FixtureMetadata metadata = metadataBuilder.build();
    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated TriggerSmartContract edge_unknown_selector_revert fixture");
  }

  // ==========================================================================
  // Phase 3: StorageDemo Boundary Fixtures
  // ==========================================================================

  @Test
  public void generateTriggerSmartContract_edgeLongStringGt31StoreAndRead() throws Exception {
    log.info("Generating TriggerSmartContract edge_long_string_gt_31_store_and_read fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Capture pre-state
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/edge_long_string_gt_31_store_and_read");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Store a string > 31 bytes (32 chars = 32 bytes)
    // "abcdefghijklmnopqrstuvwxyz123456" = 32 chars
    String longString = "abcdefghijklmnopqrstuvwxyz123456";
    byte[] longStringBytes = longString.getBytes();
    Assert.assertTrue("String must be > 31 bytes", longStringBytes.length > 31);

    // Build ABI-encoded params for testPut(1, "abcdefghijklmnopqrstuvwxyz123456")
    // uint256 key = 1
    // offset to string data = 0x40 (64)
    // string length = 32 (0x20)
    // string data = "abcdefghijklmnopqrstuvwxyz123456" padded to 32 bytes
    String params = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000020"
        + Hex.toHexString(longStringBytes);

    byte[] triggerData = TvmTestUtils.parseAbi("testPut(uint256,string)", params);

    TVMTestResult result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, triggerData, 0, DEFAULT_FEE_LIMIT, manager, null);

    Assert.assertNull("Long string store should succeed", result.getRuntime().getRuntimeError());
    log.info("Long string (> 31 bytes) stored successfully");

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt
    if (result.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        result.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("edge_long_string_gt_31_store_and_read")
        .caseCategory("edge")
        .description("Store string > 31 bytes (triggers multi-slot storage layout)")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("storage-row")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    metadata.setExpectedStatus("SUCCESS");
    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated edge_long_string_gt_31_store_and_read fixture");
  }

  @Test
  public void generateTriggerSmartContract_edgeDeleteNonexistentKeyNoop() throws Exception {
    log.info("Generating TriggerSmartContract edge_delete_nonexistent_key_noop fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Capture pre-state (do NOT write anything first)
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/edge_delete_nonexistent_key_noop");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Delete key that was never set (key = 999)
    String deleteParams = "00000000000000000000000000000000000000000000000000000000000003e7";
    byte[] deleteData = TvmTestUtils.parseAbi("testDelete(uint256)", deleteParams);

    TVMTestResult deleteResult = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, deleteData, 0, DEFAULT_FEE_LIMIT, manager, null);

    Assert.assertNull("Delete nonexistent should succeed (no-op)",
        deleteResult.getRuntime().getRuntimeError());

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(deleteData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt
    if (deleteResult.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        deleteResult.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("edge_delete_nonexistent_key_noop")
        .caseCategory("edge")
        .description("Delete storage key that was never set (should be no-op)")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("storage-row")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    metadata.setExpectedStatus("SUCCESS");
    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated edge_delete_nonexistent_key_noop fixture");
  }

  @Test
  public void generateTriggerSmartContract_edgeReadNonexistentKeyReturnsEmpty() throws Exception {
    log.info("Generating TriggerSmartContract edge_read_nonexistent_key_returns_empty fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Capture pre-state
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/edge_read_nonexistent_key_returns_empty");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Read key that was never set (key = 888)
    String viewParams = "0000000000000000000000000000000000000000000000000000000000000378";
    byte[] viewData = TvmTestUtils.parseAbi("int2str(uint256)", viewParams);

    TVMTestResult viewResult = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, viewData, 0, DEFAULT_FEE_LIMIT, manager, null);

    Assert.assertNull("Read nonexistent should succeed (returns empty)",
        viewResult.getRuntime().getRuntimeError());

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(viewData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt
    if (viewResult.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        viewResult.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("edge_read_nonexistent_key_returns_empty")
        .caseCategory("edge")
        .description("Read storage key that was never set (returns empty string)")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    metadata.setExpectedStatus("SUCCESS");
    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated edge_read_nonexistent_key_returns_empty fixture");
  }

  @Test
  public void generateTriggerSmartContract_edgePutEmptyString() throws Exception {
    log.info("Generating TriggerSmartContract edge_put_empty_string fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Capture pre-state
    File fixtureDir = new File(outputDir, "trigger_smart_contract/edge_put_empty_string");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Store empty string at key 1
    // testPut(1, "")
    String params = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000000"; // length = 0

    byte[] triggerData = TvmTestUtils.parseAbi("testPut(uint256,string)", params);

    TVMTestResult result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, triggerData, 0, DEFAULT_FEE_LIMIT, manager, null);

    Assert.assertNull("Put empty string should succeed", result.getRuntime().getRuntimeError());

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt
    if (result.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        result.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("edge_put_empty_string")
        .caseCategory("edge")
        .description("Store empty string value (differs from delete semantics)")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("storage-row")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    metadata.setExpectedStatus("SUCCESS");
    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated edge_put_empty_string fixture");
  }

  @Test
  public void generateTriggerSmartContract_edgeOverwriteShortToLong() throws Exception {
    log.info("Generating TriggerSmartContract edge_overwrite_short_to_long fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // First write a short string (< 31 bytes)
    String shortParams = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000005"
        + "68656c6c6f000000000000000000000000000000000000000000000000000000"; // "hello"
    byte[] shortData = TvmTestUtils.parseAbi("testPut(uint256,string)", shortParams);
    TVMTestResult shortResult = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, shortData, 0, DEFAULT_FEE_LIMIT, manager, null);
    Assert.assertNull("Short write should succeed", shortResult.getRuntime().getRuntimeError());

    // Capture pre-state for overwrite
    File fixtureDir = new File(outputDir, "trigger_smart_contract/edge_overwrite_short_to_long");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Overwrite with long string (> 31 bytes)
    String longString = "this_is_a_very_long_string_that_exceeds_31_bytes";
    byte[] longStringBytes = longString.getBytes();
    Assert.assertTrue("String must be > 31 bytes", longStringBytes.length > 31);

    // Pad to multiple of 32 bytes
    int paddedLen = ((longStringBytes.length + 31) / 32) * 32;
    byte[] paddedBytes = new byte[paddedLen];
    System.arraycopy(longStringBytes, 0, paddedBytes, 0, longStringBytes.length);

    String longParams = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + String.format("%064x", longStringBytes.length)
        + Hex.toHexString(paddedBytes);

    byte[] longData = TvmTestUtils.parseAbi("testPut(uint256,string)", longParams);
    TVMTestResult longResult = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, longData, 0, DEFAULT_FEE_LIMIT, manager, null);
    Assert.assertNull("Long overwrite should succeed", longResult.getRuntime().getRuntimeError());

    // Build request for the overwrite operation
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(longData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt
    if (longResult.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        longResult.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("edge_overwrite_short_to_long")
        .caseCategory("edge")
        .description("Overwrite short string (<31 bytes) with long string (>31 bytes)")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("storage-row")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    metadata.setExpectedStatus("SUCCESS");
    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated edge_overwrite_short_to_long fixture");
  }

  @Test
  public void generateTriggerSmartContract_validateFailVmDisabled() throws Exception {
    log.info("Generating TriggerSmartContract validate_fail_vm_disabled fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract first (while VM is enabled)
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Save original VM support setting
    long originalAllowCreation = manager.getDynamicPropertiesStore().getAllowCreationOfContracts();

    // Disable VM support
    manager.getDynamicPropertiesStore().saveAllowCreationOfContracts(0);

    // Capture pre-state
    File fixtureDir = new File(outputDir, "trigger_smart_contract/validate_fail_vm_disabled");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    String expectedError = "VM work is off, need to be opened by the committee";
    String errorMessage = null;
    TVMTestResult result = null;

    try {
      String params = "0000000000000000000000000000000000000000000000000000000000000001"
          + "0000000000000000000000000000000000000000000000000000000000000040"
          + "0000000000000000000000000000000000000000000000000000000000000005"
          + "68656c6c6f000000000000000000000000000000000000000000000000000000";
      byte[] triggerData = TvmTestUtils.parseAbi("testPut(uint256,string)", params);

      result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
          ownerBytes, contractAddress, triggerData, 0, DEFAULT_FEE_LIMIT, manager, null);
      if (result.getRuntime().getRuntimeError() != null) {
        errorMessage = result.getRuntime().getRuntimeError();
      }
    } catch (org.tron.core.exception.ContractValidateException e) {
      errorMessage = e.getMessage();
      log.info("Got expected validation error: {}", errorMessage);
    } catch (Exception e) {
      errorMessage = e.getMessage();
    } finally {
      // Restore original VM support setting
      manager.getDynamicPropertiesStore().saveAllowCreationOfContracts(originalAllowCreation);
    }

    log.info("VM disabled trigger result: error={}", errorMessage);

    // Build request
    String params = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000005"
        + "68656c6c6f000000000000000000000000000000000000000000000000000000";
    byte[] triggerData = TvmTestUtils.parseAbi("testPut(uint256,string)", params);

    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("validate_fail_vm_disabled")
        .caseCategory("validate_fail")
        .description("Trigger when VM support is disabled via committee")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError(expectedError)
        .build();

    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated TriggerSmartContract validate_fail_vm_disabled fixture");
  }

  @Test
  public void generateTriggerSmartContract_validateFailOwnerAddressInvalidEmpty() throws Exception {
    log.info("Generating TriggerSmartContract validate_fail_owner_address_invalid_empty fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract first
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Capture pre-state
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/validate_fail_owner_address_invalid_empty");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Build trigger with empty owner address
    String params = "0000000000000000000000000000000000000000000000000000000000000001";
    byte[] triggerData = TvmTestUtils.parseAbi("testDelete(uint256)", params);

    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.EMPTY)
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .build();

    // Note: The actual execution with empty owner may fail in different ways
    // depending on where validation happens. We capture the expected behavior.
    String expectedError = "Invalid ownerAddress";

    log.info("Empty owner address fixture: expected error = {}", expectedError);

    // Save request
    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("validate_fail_owner_address_invalid_empty")
        .caseCategory("validate_fail")
        .description("Trigger with empty owner address should fail validation")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress("")
        .expectedError(expectedError)
        .build();

    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated validate_fail_owner_address_invalid_empty fixture");
  }

  @Test
  public void generateTriggerSmartContract_validateFailOwnerAccountMissing() throws Exception {
    log.info("Generating TriggerSmartContract validate_fail_owner_account_missing fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract first
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Capture pre-state
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/validate_fail_owner_account_missing");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Use a valid-looking address that doesn't exist in AccountStore
    // TRON addresses are 21 bytes: 0x41 prefix + 20 bytes
    byte[] missingOwner = Hex.decode("41aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

    String params = "0000000000000000000000000000000000000000000000000000000000000001";
    byte[] triggerData = TvmTestUtils.parseAbi("testDelete(uint256)", params);

    String errorMessage = null;
    TVMTestResult result = null;

    try {
      result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
          missingOwner, contractAddress, triggerData, 0, DEFAULT_FEE_LIMIT, manager, null);
      if (result.getRuntime().getRuntimeError() != null) {
        errorMessage = result.getRuntime().getRuntimeError();
      }
    } catch (org.tron.core.exception.ContractValidateException e) {
      errorMessage = e.getMessage();
      log.info("Got expected validation error: {}", errorMessage);
    } catch (Exception e) {
      errorMessage = e.getMessage();
    }

    log.info("Missing owner account trigger result: error={}", errorMessage);

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(missingOwner))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save metadata
    String expectedError = errorMessage != null ? errorMessage : "Account not exists";
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("validate_fail_owner_account_missing")
        .caseCategory("validate_fail")
        .description("Trigger with owner address that does not exist in AccountStore")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(Hex.toHexString(missingOwner))
        .expectedError(expectedError)
        .build();

    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated validate_fail_owner_account_missing fixture");
  }

  @Test
  public void generateTriggerSmartContract_validateFailContractAddressMissing() throws Exception {
    log.info("Generating TriggerSmartContract validate_fail_contract_address_missing fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Capture pre-state
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/validate_fail_contract_address_missing");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Build trigger without contract address (empty)
    String params = "0000000000000000000000000000000000000000000000000000000000000001";
    byte[] triggerData = TvmTestUtils.parseAbi("testDelete(uint256)", params);

    String expectedError = "Cannot get contract address from TriggerContract";

    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.EMPTY) // Missing contract address
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .build();

    log.info("Missing contract address fixture: expected error = {}", expectedError);

    // Save request
    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("validate_fail_contract_address_missing")
        .caseCategory("validate_fail")
        .description("Trigger without contract address should fail validation")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError(expectedError)
        .build();

    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated validate_fail_contract_address_missing fixture");
  }

  @Test
  public void generateTriggerSmartContract_validateFailContractAddressInvalidBytes()
      throws Exception {
    log.info("Generating TriggerSmartContract validate_fail_contract_address_invalid_bytes");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Capture pre-state
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/validate_fail_contract_address_invalid_bytes");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Use invalid address bytes (wrong length - 10 bytes instead of 21)
    byte[] invalidContractAddress = Hex.decode("01020304050607080910");

    String params = "0000000000000000000000000000000000000000000000000000000000000001";
    byte[] triggerData = TvmTestUtils.parseAbi("testDelete(uint256)", params);

    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(invalidContractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .build();

    String expectedError = "Invalid contract address";

    log.info("Invalid contract address bytes fixture: expected error = {}", expectedError);

    // Save request
    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("validate_fail_contract_address_invalid_bytes")
        .caseCategory("validate_fail")
        .description("Trigger with invalid contract address bytes (wrong length)")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError(expectedError)
        .build();

    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated validate_fail_contract_address_invalid_bytes fixture");
  }

  @Test
  public void generateTriggerSmartContract_validateFailCallValueInsufficientBalance()
      throws Exception {
    log.info("Generating TriggerSmartContract validate_fail_call_value_insufficient_balance");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract first
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Get owner's current balance
    long ownerBalance = manager.getAccountStore()
        .get(ownerBytes).getBalance();
    log.info("Owner balance before trigger: {} SUN", ownerBalance);

    // Capture pre-state
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/validate_fail_call_value_insufficient_balance");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Try to trigger with callValue much greater than balance
    long excessiveCallValue = ownerBalance + 1_000_000_000_000L; // Way more than balance
    String params = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000005"
        + "68656c6c6f000000000000000000000000000000000000000000000000000000";
    byte[] triggerData = TvmTestUtils.parseAbi("testPut(uint256,string)", params);

    String errorMessage = null;
    TVMTestResult result = null;

    try {
      result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
          ownerBytes, contractAddress, triggerData, excessiveCallValue,
          DEFAULT_FEE_LIMIT, manager, null);
      if (result.getRuntime().getRuntimeError() != null) {
        errorMessage = result.getRuntime().getRuntimeError();
      }
    } catch (org.tron.core.exception.ContractValidateException e) {
      errorMessage = e.getMessage();
      log.info("Got expected validation error: {}", errorMessage);
    } catch (Exception e) {
      errorMessage = e.getMessage();
    }

    log.info("Insufficient balance trigger result: error={}", errorMessage);

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(excessiveCallValue)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save metadata
    String expectedError = errorMessage != null ? errorMessage : "balance is not sufficient";
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("validate_fail_call_value_insufficient_balance")
        .caseCategory("validate_fail")
        .description("Trigger with callValue greater than owner balance")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError(expectedError)
        .build();

    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated validate_fail_call_value_insufficient_balance fixture");
  }

  @Test
  public void generateTriggerSmartContract_validateFailCallValueNegative() throws Exception {
    log.info("Generating TriggerSmartContract validate_fail_call_value_negative fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract first
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Capture pre-state
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/validate_fail_call_value_negative");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Try to trigger with negative callValue
    long negativeCallValue = -1L;
    String params = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000005"
        + "68656c6c6f000000000000000000000000000000000000000000000000000000";
    byte[] triggerData = TvmTestUtils.parseAbi("testPut(uint256,string)", params);

    String errorMessage = null;
    TVMTestResult result = null;

    try {
      result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
          ownerBytes, contractAddress, triggerData, negativeCallValue,
          DEFAULT_FEE_LIMIT, manager, null);
      if (result.getRuntime().getRuntimeError() != null) {
        errorMessage = result.getRuntime().getRuntimeError();
      }
    } catch (org.tron.core.exception.ContractValidateException e) {
      errorMessage = e.getMessage();
      log.info("Got expected validation error: {}", errorMessage);
    } catch (Exception e) {
      errorMessage = e.getMessage();
    }

    log.info("Negative callValue trigger result: error={}", errorMessage);

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(negativeCallValue)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save metadata
    // Error may be "callValue must be >= 0" (with ENERGY_LIMIT_HARD_FORK)
    // or "Amount must be greater than or equals 0." (without hard fork)
    String expectedError = errorMessage != null ? errorMessage : "callValue must be >= 0";
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("validate_fail_call_value_negative")
        .caseCategory("validate_fail")
        .description("Trigger with negative callValue should fail validation")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError(expectedError)
        .build();

    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated validate_fail_call_value_negative fixture");
  }

  @Test
  public void generateTriggerSmartContract_validateFailTokenAssetMissing() throws Exception {
    log.info("Generating TriggerSmartContract validate_fail_token_asset_missing fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract first
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Enable TRC-10 transfer and multi-sign
    manager.getDynamicPropertiesStore().saveAllowTvmTransferTrc10(1);
    manager.getDynamicPropertiesStore().saveAllowMultiSign(1);

    // Capture pre-state
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/validate_fail_token_asset_missing");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Try with valid tokenId that doesn't exist as an asset
    String params = "0000000000000000000000000000000000000000000000000000000000000001";
    byte[] triggerData = TvmTestUtils.parseAbi("testDelete(uint256)", params);

    long tokenValue = 100L;
    long tokenId = 1_000_001L; // Valid tokenId but asset doesn't exist
    String expectedError = "No asset !";

    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(0)
        .setCallTokenValue(tokenValue)
        .setTokenId(tokenId)
        .build();

    log.info("Missing asset fixture: expected error = {}", expectedError);

    // Save request
    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("validate_fail_token_asset_missing")
        .caseCategory("validate_fail")
        .description("Trigger with tokenId for non-existent asset should fail")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .expectedError(expectedError)
        .build();

    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated validate_fail_token_asset_missing fixture");
  }

  @Test
  public void generateTriggerSmartContract_edgeNonpayableWithCallValueRevert() throws Exception {
    log.info("Generating TriggerSmartContract edge_nonpayable_with_call_value_revert fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract first (StorageDemo functions are nonpayable)
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // Capture pre-state
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/edge_nonpayable_with_call_value_revert");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Call nonpayable function with callValue > 0 (should revert)
    String params = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000005"
        + "68656c6c6f000000000000000000000000000000000000000000000000000000";
    byte[] triggerData = TvmTestUtils.parseAbi("testPut(uint256,string)", params);

    long callValue = 1_000_000L; // 1 TRX
    String errorMessage = null;
    TVMTestResult result = null;

    try {
      result = TvmTestUtils.triggerContractAndReturnTvmTestResult(
          ownerBytes, contractAddress, triggerData, callValue, DEFAULT_FEE_LIMIT, manager, null);
      if (result.getRuntime().getRuntimeError() != null) {
        errorMessage = result.getRuntime().getRuntimeError();
      }
    } catch (Exception e) {
      errorMessage = e.getMessage();
    }

    log.info("Nonpayable with callValue trigger result: error={}", errorMessage);

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(triggerData))
        .setCallValue(callValue)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt if available
    if (result != null && result.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        result.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata.Builder metadataBuilder = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("edge_nonpayable_with_call_value_revert")
        .caseCategory("edge")
        .description("Call nonpayable function with callValue > 0 (should revert)")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS);

    if (errorMessage != null) {
      metadataBuilder.expectedRevert(errorMessage);
    } else {
      // If no error, it may have succeeded (depending on contract/compiler)
      metadataBuilder.expectedStatus("SUCCESS");
    }

    FixtureMetadata metadata = metadataBuilder.build();
    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated edge_nonpayable_with_call_value_revert fixture");
  }

  @Test
  public void generateTriggerSmartContract_edgeDeleteLongStringRefund() throws Exception {
    log.info("Generating TriggerSmartContract edge_delete_long_string_refund fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // First write a long string (> 31 bytes) to create multi-slot storage
    String longString = "this_is_a_very_long_string_that_exceeds_31_bytes_and_uses_multiple_slots";
    byte[] longStringBytes = longString.getBytes();
    int paddedLen = ((longStringBytes.length + 31) / 32) * 32;
    byte[] paddedBytes = new byte[paddedLen];
    System.arraycopy(longStringBytes, 0, paddedBytes, 0, longStringBytes.length);

    String longParams = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + String.format("%064x", longStringBytes.length)
        + Hex.toHexString(paddedBytes);

    byte[] longData = TvmTestUtils.parseAbi("testPut(uint256,string)", longParams);
    TVMTestResult longResult = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, longData, 0, DEFAULT_FEE_LIMIT, manager, null);
    Assert.assertNull("Long write should succeed", longResult.getRuntime().getRuntimeError());

    // Capture pre-state for delete
    File fixtureDir = new File(outputDir,
        "trigger_smart_contract/edge_delete_long_string_refund");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Delete the long string (should clean up multi-slot storage and trigger refund)
    String deleteParams = "0000000000000000000000000000000000000000000000000000000000000001";
    byte[] deleteData = TvmTestUtils.parseAbi("testDelete(uint256)", deleteParams);
    TVMTestResult deleteResult = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, deleteData, 0, DEFAULT_FEE_LIMIT, manager, null);
    Assert.assertNull("Delete should succeed", deleteResult.getRuntime().getRuntimeError());

    // Build request
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(deleteData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt
    if (deleteResult.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        deleteResult.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("edge_delete_long_string_refund")
        .caseCategory("edge")
        .description("Delete long string (>31 bytes) to verify storage cleanup and refund")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("storage-row")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    metadata.setExpectedStatus("SUCCESS");
    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated edge_delete_long_string_refund fixture");
  }

  @Test
  public void generateTriggerSmartContract_edgeOverwriteLongToShort() throws Exception {
    log.info("Generating TriggerSmartContract edge_overwrite_long_to_short fixture");

    byte[] ownerBytes = Hex.decode(OWNER_ADDRESS);

    // Deploy contract
    Transaction deployTx = TvmTestUtils.generateDeploySmartContractAndGetTransaction(
        "StorageDemo", ownerBytes, STORAGE_ABI, STORAGE_CODE,
        0, DEFAULT_FEE_LIMIT, 50, null);

    byte[] contractAddress = WalletUtil.generateContractAddress(deployTx);
    runtime = TvmTestUtils.processTransactionAndReturnRuntime(deployTx, rootRepository, null);
    Assert.assertNull("Deploy should succeed", runtime.getRuntimeError());

    // First write a long string (> 31 bytes)
    String longString = "this_is_a_very_long_string_that_exceeds_31_bytes";
    byte[] longStringBytes = longString.getBytes();
    int paddedLen = ((longStringBytes.length + 31) / 32) * 32;
    byte[] paddedBytes = new byte[paddedLen];
    System.arraycopy(longStringBytes, 0, paddedBytes, 0, longStringBytes.length);

    String longParams = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + String.format("%064x", longStringBytes.length)
        + Hex.toHexString(paddedBytes);

    byte[] longData = TvmTestUtils.parseAbi("testPut(uint256,string)", longParams);
    TVMTestResult longResult = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, longData, 0, DEFAULT_FEE_LIMIT, manager, null);
    Assert.assertNull("Long write should succeed", longResult.getRuntime().getRuntimeError());

    // Capture pre-state for overwrite
    File fixtureDir = new File(outputDir, "trigger_smart_contract/edge_overwrite_long_to_short");
    fixtureDir.mkdirs();
    File preDbDir = new File(fixtureDir, "pre_db");
    preDbDir.mkdirs();
    captureVmDatabases(preDbDir);

    // Overwrite with short string (< 31 bytes)
    String shortParams = "0000000000000000000000000000000000000000000000000000000000000001"
        + "0000000000000000000000000000000000000000000000000000000000000040"
        + "0000000000000000000000000000000000000000000000000000000000000003"
        + "6162630000000000000000000000000000000000000000000000000000000000"; // "abc"
    byte[] shortData = TvmTestUtils.parseAbi("testPut(uint256,string)", shortParams);
    TVMTestResult shortResult = TvmTestUtils.triggerContractAndReturnTvmTestResult(
        ownerBytes, contractAddress, shortData, 0, DEFAULT_FEE_LIMIT, manager, null);
    Assert.assertNull("Short overwrite should succeed", shortResult.getRuntime().getRuntimeError());

    // Build request for the overwrite operation
    TriggerSmartContract triggerContract = TriggerSmartContract.newBuilder()
        .setOwnerAddress(ByteString.copyFrom(ownerBytes))
        .setContractAddress(ByteString.copyFrom(contractAddress))
        .setData(ByteString.copyFrom(shortData))
        .setCallValue(0)
        .build();

    ExecuteTransactionRequest request = buildTriggerRequest(triggerContract, null);
    File requestFile = new File(fixtureDir, "request.pb");
    try (FileOutputStream fos = new FileOutputStream(requestFile)) {
      request.writeTo(fos);
    }

    // Capture post-state
    File expectedDir = new File(fixtureDir, "expected");
    File postDbDir = new File(expectedDir, "post_db");
    postDbDir.mkdirs();
    captureVmDatabases(postDbDir);

    // Save receipt
    if (shortResult.getReceipt() != null) {
      File resultFile = new File(expectedDir, "result.pb");
      try (FileOutputStream fos = new FileOutputStream(resultFile)) {
        shortResult.getReceipt().getReceipt().writeTo(fos);
      }
    }

    // Save metadata
    FixtureMetadata metadata = FixtureMetadata.builder()
        .contractType("TRIGGER_SMART_CONTRACT", 31)
        .caseName("edge_overwrite_long_to_short")
        .caseCategory("edge")
        .description("Overwrite long string (>31 bytes) with short string (<31 bytes)")
        .database("account")
        .database("contract")
        .database("code")
        .database("abi")
        .database("contract-state")
        .database("storage-row")
        .database("dynamic-properties")
        .ownerAddress(OWNER_ADDRESS)
        .build();

    metadata.setExpectedStatus("SUCCESS");
    metadata.toFile(new File(fixtureDir, "metadata.json"));
    log.info("Generated edge_overwrite_long_to_short fixture");
  }

  // ==========================================================================
  // Helper Methods
  // ==========================================================================

  private ExecuteTransactionRequest buildTriggerRequest(
      TriggerSmartContract triggerContract,
      BlockCapsule blockCap) {

    byte[] fromAddress = triggerContract.getOwnerAddress().toByteArray();
    byte[] toAddress = triggerContract.getContractAddress().toByteArray();
    byte[] data = triggerContract.toByteArray();
    long value = triggerContract.getCallValue();

    TronTransaction tronTx = TronTransaction.newBuilder()
        .setFrom(ByteString.copyFrom(fromAddress))
        .setTo(ByteString.copyFrom(toAddress))
        .setValue(ByteString.copyFrom(longToBytes32(value)))
        .setData(ByteString.copyFrom(data))
        .setEnergyLimit(DEFAULT_FEE_LIMIT)
        .setEnergyPrice(1)
        .setNonce(0)
        .setTxKind(TxKind.VM)
        .setContractType(ContractType.TRIGGER_SMART_CONTRACT)
        .build();

    ExecutionContext.Builder contextBuilder = ExecutionContext.newBuilder()
        .setEnergyLimit(DEFAULT_FEE_LIMIT)
        .setEnergyPrice(1);

    if (blockCap != null) {
      contextBuilder
          .setBlockNumber(blockCap.getNum())
          .setBlockTimestamp(blockCap.getTimeStamp())
          .setBlockHash(ByteString.copyFrom(blockCap.getBlockId().getBytes()))
          .setCoinbase(ByteString.copyFrom(blockCap.getWitnessAddress().toByteArray()));
    } else {
      // Use deterministic timestamps for fixture generation
      contextBuilder
          .setBlockNumber(FIXED_BLOCK_NUMBER)
          .setBlockTimestamp(FIXED_BLOCK_TIMESTAMP);
    }

    return ExecuteTransactionRequest.newBuilder()
        .setTransaction(tronTx)
        .setContext(contextBuilder.build())
        .build();
  }

  private ExecuteTransactionRequest buildTriggerRequestWithFeeLimit(
      TriggerSmartContract triggerContract,
      BlockCapsule blockCap,
      long feeLimit) {

    byte[] fromAddress = triggerContract.getOwnerAddress().toByteArray();
    byte[] toAddress = triggerContract.getContractAddress().toByteArray();
    byte[] data = triggerContract.toByteArray();
    long value = triggerContract.getCallValue();

    TronTransaction tronTx = TronTransaction.newBuilder()
        .setFrom(ByteString.copyFrom(fromAddress))
        .setTo(ByteString.copyFrom(toAddress))
        .setValue(ByteString.copyFrom(longToBytes32(value)))
        .setData(ByteString.copyFrom(data))
        .setEnergyLimit(feeLimit)
        .setEnergyPrice(1)
        .setNonce(0)
        .setTxKind(TxKind.VM)
        .setContractType(ContractType.TRIGGER_SMART_CONTRACT)
        .build();

    ExecutionContext.Builder contextBuilder = ExecutionContext.newBuilder()
        .setEnergyLimit(feeLimit)
        .setEnergyPrice(1);

    if (blockCap != null) {
      contextBuilder
          .setBlockNumber(blockCap.getNum())
          .setBlockTimestamp(blockCap.getTimeStamp())
          .setBlockHash(ByteString.copyFrom(blockCap.getBlockId().getBytes()))
          .setCoinbase(ByteString.copyFrom(blockCap.getWitnessAddress().toByteArray()));
    } else {
      // Use deterministic timestamps for fixture generation
      contextBuilder
          .setBlockNumber(FIXED_BLOCK_NUMBER)
          .setBlockTimestamp(FIXED_BLOCK_TIMESTAMP);
    }

    return ExecuteTransactionRequest.newBuilder()
        .setTransaction(tronTx)
        .setContext(contextBuilder.build())
        .build();
  }

  private void captureVmDatabases(File outputDir) throws Exception {
    String[] databases = {
        "account",
        "contract",
        "code",
        "abi",
        "contract-state",
        "storage-row",
        "dynamic-properties"
    };

    for (String dbName : databases) {
      SortedMap<byte[], byte[]> kvData = new TreeMap<>(KvFileFormat.BYTE_ARRAY_COMPARATOR);

      try {
        Iterator<Map.Entry<byte[], byte[]>> iterator = getStoreIterator(dbName);
        if (iterator != null) {
          while (iterator.hasNext()) {
            Map.Entry<byte[], byte[]> entry = iterator.next();
            kvData.put(entry.getKey(), entry.getValue());
          }
        }
      } catch (Exception e) {
        log.warn("Failed to capture state for database {}: {}", dbName, e.getMessage());
      }

      File kvFile = new File(outputDir, dbName + ".kv");
      KvFileFormat.write(kvFile, kvData);
      log.debug("Captured {} entries from {}", kvData.size(), dbName);
    }
  }

  @SuppressWarnings("unchecked")
  private Iterator<Map.Entry<byte[], byte[]>> getStoreIterator(String dbName) {
    try {
      switch (dbName) {
        case "account":
          return convertIterator(manager.getAccountStore().iterator());
        case "contract":
          return convertIterator(manager.getContractStore().iterator());
        case "code":
          return convertIterator(manager.getCodeStore().iterator());
        case "abi":
          return convertIterator(manager.getChainBaseManager().getAbiStore().iterator());
        case "contract-state":
          return convertIterator(manager.getChainBaseManager().getContractStateStore().iterator());
        case "storage-row":
          return convertIterator(manager.getChainBaseManager().getStorageRowStore().iterator());
        case "dynamic-properties":
          return convertIterator(manager.getDynamicPropertiesStore().iterator());
        default:
          log.warn("Unknown database: {}", dbName);
          return null;
      }
    } catch (Exception e) {
      log.warn("Failed to get iterator for {}: {}", dbName, e.getMessage());
      return null;
    }
  }

  @SuppressWarnings("unchecked")
  private Iterator<Map.Entry<byte[], byte[]>> convertIterator(Iterator<?> storeIterator) {
    java.util.List<Map.Entry<byte[], byte[]>> entries = new java.util.ArrayList<>();
    while (storeIterator.hasNext()) {
      Object entry = storeIterator.next();
      if (entry instanceof Map.Entry) {
        Map.Entry<?, ?> mapEntry = (Map.Entry<?, ?>) entry;
        byte[] key = (byte[]) mapEntry.getKey();
        Object value = mapEntry.getValue();

        byte[] valueBytes;
        if (value instanceof byte[]) {
          valueBytes = (byte[]) value;
        } else if (value instanceof org.tron.core.capsule.ProtoCapsule) {
          valueBytes = ((org.tron.core.capsule.ProtoCapsule<?>) value).getData();
        } else if (value != null) {
          continue;
        } else {
          valueBytes = new byte[0];
        }

        entries.add(new java.util.AbstractMap.SimpleEntry<>(key, valueBytes));
      }
    }
    return entries.iterator();
  }

  private byte[] longToBytes32(long value) {
    byte[] result = new byte[32];
    for (int i = 7; i >= 0; i--) {
      result[31 - i] = (byte) (value >>> (i * 8));
    }
    return result;
  }
}

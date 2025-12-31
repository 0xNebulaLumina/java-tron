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
      contextBuilder
          .setBlockNumber(1)
          .setBlockTimestamp(System.currentTimeMillis());
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
      contextBuilder
          .setBlockNumber(1)
          .setBlockTimestamp(System.currentTimeMillis());
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

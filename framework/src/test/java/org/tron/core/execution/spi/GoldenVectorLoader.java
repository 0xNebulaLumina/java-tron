package org.tron.core.execution.spi;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.google.protobuf.Any;
import com.google.protobuf.ByteString;
import java.io.IOException;
import java.io.InputStream;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.utils.ByteArray;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.protos.Protocol;
import org.tron.protos.contract.BalanceContract;
import org.tron.protos.contract.SmartContractOuterClass;

/** Loader for golden vectors from JSON configuration files. */
public class GoldenVectorLoader {

  private static final Logger logger = LoggerFactory.getLogger(GoldenVectorLoader.class);
  private static final ObjectMapper objectMapper = new ObjectMapper();

  /** Load golden vectors from the default configuration file. */
  public static List<GoldenVector> loadDefaultVectors() {
    return loadVectors("/golden-vectors.json");
  }

  /** Load golden vectors from a specific configuration file. */
  public static List<GoldenVector> loadVectors(String resourcePath) {
    try (InputStream inputStream = GoldenVectorLoader.class.getResourceAsStream(resourcePath)) {
      if (inputStream == null) {
        throw new RuntimeException("Golden vector configuration not found: " + resourcePath);
      }

      JsonNode root = objectMapper.readTree(inputStream);
      return parseGoldenVectors(root);

    } catch (IOException e) {
      throw new RuntimeException("Failed to load golden vectors from " + resourcePath, e);
    }
  }

  /** Parse golden vectors from JSON configuration. */
  private static List<GoldenVector> parseGoldenVectors(JsonNode root) {
    List<GoldenVector> vectors = new ArrayList<>();

    // Parse test addresses
    Map<String, String> testAddresses = parseTestAddresses(root.get("test_addresses"));

    // Parse vectors
    JsonNode vectorsNode = root.get("vectors");
    if (vectorsNode != null && vectorsNode.isArray()) {
      for (JsonNode vectorNode : vectorsNode) {
        try {
          GoldenVector vector = parseGoldenVector(vectorNode, testAddresses);
          vectors.add(vector);
        } catch (Exception e) {
          logger.error("Failed to parse golden vector: {}", vectorNode.get("name"), e);
        }
      }
    }

    logger.info("Loaded {} golden vectors from configuration", vectors.size());
    return vectors;
  }

  /** Parse test addresses mapping. */
  private static Map<String, String> parseTestAddresses(JsonNode addressesNode) {
    Map<String, String> addresses = new HashMap<>();
    if (addressesNode != null) {
      addressesNode
          .fields()
          .forEachRemaining(entry -> addresses.put(entry.getKey(), entry.getValue().asText()));
    }
    return addresses;
  }

  /** Parse a single golden vector from JSON. */
  private static GoldenVector parseGoldenVector(
      JsonNode vectorNode, Map<String, String> testAddresses) {
    String name = vectorNode.get("name").asText();
    String category = vectorNode.get("category").asText();
    String description =
        vectorNode.has("description") ? vectorNode.get("description").asText() : "";
    String transactionType = vectorNode.get("transaction_type").asText();

    // Parse transaction parameters
    JsonNode parametersNode = vectorNode.get("parameters");
    TransactionCapsule transaction =
        createTransaction(transactionType, parametersNode, testAddresses);

    // Parse expected results
    JsonNode expectedNode = vectorNode.get("expected");
    GoldenVector.ExpectedResult expectedResult = parseExpectedResult(expectedNode);

    // Determine if it's a contract call
    boolean isContractCall = transactionType.equals("TriggerSmartContract");

    return new GoldenVector(
        name, category, transaction, isContractCall, expectedResult, description);
  }

  /** Create a transaction based on type and parameters. */
  private static TransactionCapsule createTransaction(
      String transactionType, JsonNode parameters, Map<String, String> testAddresses) {
    try {
      switch (transactionType) {
        case "TransferContract":
          return createTransferTransaction(parameters, testAddresses);
        case "TriggerSmartContract":
          return createTriggerSmartContractTransaction(parameters, testAddresses);
        case "CreateSmartContract":
          return createCreateSmartContractTransaction(parameters, testAddresses);
        case "TransferAssetContract":
          return createTransferAssetTransaction(parameters, testAddresses);
        case "FreezeBalanceV2Contract":
          return createFreezeBalanceV2Transaction(parameters, testAddresses);
        case "UnfreezeBalanceV2Contract":
          return createUnfreezeBalanceV2Transaction(parameters, testAddresses);
        case "DelegateResourceContract":
          return createDelegateResourceTransaction(parameters, testAddresses);
        case "VoteWitnessContract":
          return createVoteWitnessTransaction(parameters, testAddresses);
        case "ShieldedTransferContract":
          return createShieldedTransferTransaction(parameters, testAddresses);
        case "MarketSellAssetContract":
          return createMarketSellAssetTransaction(parameters, testAddresses);
        default:
          throw new UnsupportedOperationException(
              "Unsupported transaction type: " + transactionType);
      }
    } catch (Exception e) {
      throw new RuntimeException("Failed to create transaction of type " + transactionType, e);
    }
  }

  /** Create a transfer transaction. */
  private static TransactionCapsule createTransferTransaction(
      JsonNode parameters, Map<String, String> testAddresses) {
    String fromAddress = resolveAddress(parameters.get("from").asText(), testAddresses);
    String toAddress = resolveAddress(parameters.get("to").asText(), testAddresses);
    long amount = parameters.get("amount").asLong();

    BalanceContract.TransferContract.Builder transferBuilder =
        BalanceContract.TransferContract.newBuilder()
            .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(fromAddress)))
            .setToAddress(ByteString.copyFrom(ByteArray.fromHexString(toAddress)))
            .setAmount(amount);

    Protocol.Transaction.raw.Builder rawBuilder =
        Protocol.Transaction.raw
            .newBuilder()
            .addContract(
                Protocol.Transaction.Contract.newBuilder()
                    .setType(Protocol.Transaction.Contract.ContractType.TransferContract)
                    .setParameter(Any.pack(transferBuilder.build())))
            .setTimestamp(System.currentTimeMillis())
            .setExpiration(System.currentTimeMillis() + 60000);

    // Handle special cases
    if (parameters.has("expiration") && parameters.get("expiration").asLong() < 0) {
      rawBuilder.setExpiration(System.currentTimeMillis() + parameters.get("expiration").asLong());
    }

    if (parameters.has("data_size")) {
      int dataSize = parameters.get("data_size").asInt();
      byte[] largeData = new byte[dataSize];
      rawBuilder.setData(ByteString.copyFrom(largeData));
    }

    Protocol.Transaction.Builder txBuilder =
        Protocol.Transaction.newBuilder().setRawData(rawBuilder);

    // Handle invalid signature case
    if (parameters.has("invalid_signature") && parameters.get("invalid_signature").asBoolean()) {
      txBuilder.addSignature(ByteString.copyFrom(new byte[64])); // Invalid signature
    }

    return new TransactionCapsule(txBuilder.build());
  }

  /** Create a trigger smart contract transaction. */
  private static TransactionCapsule createTriggerSmartContractTransaction(
      JsonNode parameters, Map<String, String> testAddresses) {
    String fromAddress = resolveAddress(parameters.get("from").asText(), testAddresses);
    String contractAddress = resolveAddress(parameters.get("contract").asText(), testAddresses);
    String method = parameters.get("method").asText();
    long callValue = parameters.has("call_value") ? parameters.get("call_value").asLong() : 0;

    // Encode method call (simplified)
    byte[] data = encodeMethodCall(method, parameters.get("params"));

    SmartContractOuterClass.TriggerSmartContract.Builder triggerBuilder =
        SmartContractOuterClass.TriggerSmartContract.newBuilder()
            .setOwnerAddress(ByteString.copyFrom(ByteArray.fromHexString(fromAddress)))
            .setContractAddress(ByteString.copyFrom(ByteArray.fromHexString(contractAddress)))
            .setData(ByteString.copyFrom(data))
            .setCallValue(callValue);

    Protocol.Transaction.raw.Builder rawBuilder =
        Protocol.Transaction.raw
            .newBuilder()
            .addContract(
                Protocol.Transaction.Contract.newBuilder()
                    .setType(Protocol.Transaction.Contract.ContractType.TriggerSmartContract)
                    .setParameter(Any.pack(triggerBuilder.build())))
            .setTimestamp(System.currentTimeMillis())
            .setExpiration(System.currentTimeMillis() + 60000);

    // Set fee limit if specified
    if (parameters.has("fee_limit")) {
      rawBuilder.setFeeLimit(parameters.get("fee_limit").asLong());
    } else {
      rawBuilder.setFeeLimit(1000000000L); // Default fee limit
    }

    return new TransactionCapsule(Protocol.Transaction.newBuilder().setRawData(rawBuilder).build());
  }

  /** Create other transaction types (placeholder implementations). */
  private static TransactionCapsule createCreateSmartContractTransaction(
      JsonNode parameters, Map<String, String> testAddresses) {
    // Placeholder - would implement actual contract creation
    throw new UnsupportedOperationException("CreateSmartContract not yet implemented in loader");
  }

  private static TransactionCapsule createTransferAssetTransaction(
      JsonNode parameters, Map<String, String> testAddresses) {
    // Placeholder - would implement TRC-10 asset transfer
    throw new UnsupportedOperationException("TransferAssetContract not yet implemented in loader");
  }

  private static TransactionCapsule createFreezeBalanceV2Transaction(
      JsonNode parameters, Map<String, String> testAddresses) {
    // Placeholder - would implement Stake 2.0 freeze
    throw new UnsupportedOperationException(
        "FreezeBalanceV2Contract not yet implemented in loader");
  }

  private static TransactionCapsule createUnfreezeBalanceV2Transaction(
      JsonNode parameters, Map<String, String> testAddresses) {
    // Placeholder - would implement Stake 2.0 unfreeze
    throw new UnsupportedOperationException(
        "UnfreezeBalanceV2Contract not yet implemented in loader");
  }

  private static TransactionCapsule createDelegateResourceTransaction(
      JsonNode parameters, Map<String, String> testAddresses) {
    // Placeholder - would implement resource delegation
    throw new UnsupportedOperationException(
        "DelegateResourceContract not yet implemented in loader");
  }

  private static TransactionCapsule createVoteWitnessTransaction(
      JsonNode parameters, Map<String, String> testAddresses) {
    // Placeholder - would implement witness voting
    throw new UnsupportedOperationException("VoteWitnessContract not yet implemented in loader");
  }

  private static TransactionCapsule createShieldedTransferTransaction(
      JsonNode parameters, Map<String, String> testAddresses) {
    // Placeholder - would implement shielded transfer
    throw new UnsupportedOperationException(
        "ShieldedTransferContract not yet implemented in loader");
  }

  private static TransactionCapsule createMarketSellAssetTransaction(
      JsonNode parameters, Map<String, String> testAddresses) {
    // Placeholder - would implement market sell order
    throw new UnsupportedOperationException(
        "MarketSellAssetContract not yet implemented in loader");
  }

  /** Parse expected result from JSON. */
  private static GoldenVector.ExpectedResult parseExpectedResult(JsonNode expectedNode) {
    boolean success = expectedNode.get("success").asBoolean();
    long energyUsed = expectedNode.get("energy_used").asLong();
    long bandwidthUsed =
        expectedNode.has("bandwidth_used") ? expectedNode.get("bandwidth_used").asLong() : 0;
    int stateChanges =
        expectedNode.has("state_changes") ? expectedNode.get("state_changes").asInt() : -1;

    byte[] returnData = null;
    if (expectedNode.has("return_data") && !expectedNode.get("return_data").isNull()) {
      String returnDataHex = expectedNode.get("return_data").asText();
      if (returnDataHex.startsWith("0x")) {
        returnData = ByteArray.fromHexString(returnDataHex.substring(2));
      }
    }

    String errorMessage = null;
    if (expectedNode.has("error_message") && !expectedNode.get("error_message").isNull()) {
      errorMessage = expectedNode.get("error_message").asText();
    }

    return new GoldenVector.ExpectedResult(
        success, energyUsed, returnData, errorMessage, stateChanges, bandwidthUsed);
  }

  /** Resolve address from alias or return as-is. */
  private static String resolveAddress(String addressOrAlias, Map<String, String> testAddresses) {
    return testAddresses.getOrDefault(addressOrAlias, addressOrAlias);
  }

  /** Encode method call for smart contract (simplified). */
  private static byte[] encodeMethodCall(String method, JsonNode params) {
    // Simplified encoding - in a real implementation, this would use proper ABI encoding
    StringBuilder encoded = new StringBuilder(method);
    if (params != null && params.isArray() && params.size() > 0) {
      encoded.append("(");
      for (int i = 0; i < params.size(); i++) {
        if (i > 0) encoded.append(",");
        encoded.append(params.get(i).asText());
      }
      encoded.append(")");
    }
    return encoded.toString().getBytes();
  }
}

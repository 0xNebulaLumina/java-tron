package org.tron.core.conformance;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import java.io.File;
import java.io.IOException;
import java.io.Reader;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.time.Instant;
import java.time.format.DateTimeFormatter;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

/**
 * Metadata for a conformance test fixture.
 *
 * <p>Captures information about the test case including contract type, category,
 * execution context, and which databases are touched during execution.
 */
public class FixtureMetadata {

  private static final Gson GSON =
      new GsonBuilder().disableHtmlEscaping().setPrettyPrinting().create();
  private static final String GENERATOR_VERSION = "1.0.0";
  private static final String STATUS_SUCCESS = "SUCCESS";
  private static final String STATUS_REVERT = "REVERT";
  private static final String STATUS_VALIDATION_FAILED = "VALIDATION_FAILED";
  private static final String STATUS_OUT_OF_ENERGY = "OUT_OF_ENERGY";

  private String contractType;
  private int contractTypeNum;
  private String caseName;
  private String caseCategory;
  private String description;
  private String generatedAt;
  private String generatorVersion;
  private long blockNumber;
  private long blockTimestamp;
  private List<String> databasesTouched;
  private String expectedStatus;
  private String expectedErrorMessage;
  private String ownerAddress;
  private Map<String, Object> dynamicProperties;
  private List<String> notes;

  public FixtureMetadata() {
    this.databasesTouched = new ArrayList<>();
    this.dynamicProperties = new TreeMap<>();
    this.notes = new ArrayList<>();
    this.generatorVersion = GENERATOR_VERSION;
    this.generatedAt = DateTimeFormatter.ISO_INSTANT.format(Instant.now());
    this.expectedStatus = STATUS_SUCCESS;
  }

  /**
   * Create metadata builder for a test case.
   */
  public static Builder builder() {
    return new Builder();
  }

  /**
   * Load metadata from a JSON file.
   */
  public static FixtureMetadata fromFile(File file) throws IOException {
    try (Reader reader = Files.newBufferedReader(file.toPath(), StandardCharsets.UTF_8)) {
      FixtureMetadata metadata = GSON.fromJson(reader, FixtureMetadata.class);
      if (metadata == null) {
        return null;
      }
      if (metadata.databasesTouched == null) {
        metadata.databasesTouched = new ArrayList<>();
      }
      if (metadata.dynamicProperties == null) {
        metadata.dynamicProperties = new TreeMap<>();
      } else if (!(metadata.dynamicProperties instanceof TreeMap)) {
        metadata.dynamicProperties = new TreeMap<>(metadata.dynamicProperties);
      }
      if (metadata.notes == null) {
        metadata.notes = new ArrayList<>();
      }
      if (metadata.expectedStatus == null) {
        metadata.expectedStatus = STATUS_SUCCESS;
      }
      return metadata;
    }
  }

  /**
   * Save metadata to a JSON file.
   */
  public void toFile(File file) throws IOException {
    try (Writer writer = Files.newBufferedWriter(file.toPath(), StandardCharsets.UTF_8)) {
      GSON.toJson(this, writer);
    }
  }

  /**
   * Convert to JSON string.
   */
  public String toJson() {
    return GSON.toJson(this);
  }

  // Getters and setters

  public String getContractType() {
    return contractType;
  }

  public void setContractType(String contractType) {
    this.contractType = contractType;
  }

  public int getContractTypeNum() {
    return contractTypeNum;
  }

  public void setContractTypeNum(int contractTypeNum) {
    this.contractTypeNum = contractTypeNum;
  }

  public String getCaseName() {
    return caseName;
  }

  public void setCaseName(String caseName) {
    this.caseName = caseName;
  }

  public String getCaseCategory() {
    return caseCategory;
  }

  public void setCaseCategory(String caseCategory) {
    this.caseCategory = caseCategory;
  }

  public String getDescription() {
    return description;
  }

  public void setDescription(String description) {
    this.description = description;
  }

  public String getGeneratedAt() {
    return generatedAt;
  }

  public void setGeneratedAt(String generatedAt) {
    this.generatedAt = generatedAt;
  }

  public String getGeneratorVersion() {
    return generatorVersion;
  }

  public void setGeneratorVersion(String generatorVersion) {
    this.generatorVersion = generatorVersion;
  }

  public long getBlockNumber() {
    return blockNumber;
  }

  public void setBlockNumber(long blockNumber) {
    this.blockNumber = blockNumber;
  }

  public long getBlockTimestamp() {
    return blockTimestamp;
  }

  public void setBlockTimestamp(long blockTimestamp) {
    this.blockTimestamp = blockTimestamp;
  }

  public List<String> getDatabasesTouched() {
    return databasesTouched;
  }

  public void setDatabasesTouched(List<String> databasesTouched) {
    this.databasesTouched = databasesTouched;
  }

  public String getExpectedStatus() {
    return expectedStatus;
  }

  public void setExpectedStatus(String expectedStatus) {
    this.expectedStatus = expectedStatus;
  }

  public String getExpectedErrorMessage() {
    return expectedErrorMessage;
  }

  public void setExpectedErrorMessage(String expectedErrorMessage) {
    this.expectedErrorMessage = expectedErrorMessage;
  }

  public String getOwnerAddress() {
    return ownerAddress;
  }

  public void setOwnerAddress(String ownerAddress) {
    this.ownerAddress = ownerAddress;
  }

  public Map<String, Object> getDynamicProperties() {
    return dynamicProperties;
  }

  public void setDynamicProperties(Map<String, Object> dynamicProperties) {
    if (dynamicProperties == null) {
      this.dynamicProperties = new TreeMap<>();
    } else if (dynamicProperties instanceof TreeMap) {
      this.dynamicProperties = dynamicProperties;
    } else {
      this.dynamicProperties = new TreeMap<>(dynamicProperties);
    }
  }

  public List<String> getNotes() {
    return notes;
  }

  public void setNotes(List<String> notes) {
    this.notes = notes;
  }

  /**
   * Builder for FixtureMetadata.
   */
  public static class Builder {
    private final FixtureMetadata metadata;

    private Builder() {
      this.metadata = new FixtureMetadata();
    }

    public Builder contractType(String type, int num) {
      metadata.setContractType(type);
      metadata.setContractTypeNum(num);
      return this;
    }

    public Builder caseName(String name) {
      metadata.setCaseName(name);
      return this;
    }

    public Builder caseCategory(String category) {
      metadata.setCaseCategory(category);
      return this;
    }

    public Builder description(String desc) {
      metadata.setDescription(desc);
      return this;
    }

    public Builder blockNumber(long num) {
      metadata.setBlockNumber(num);
      return this;
    }

    public Builder blockTimestamp(long ts) {
      metadata.setBlockTimestamp(ts);
      return this;
    }

    public Builder databases(List<String> dbs) {
      metadata.setDatabasesTouched(new ArrayList<>(dbs));
      return this;
    }

    public Builder database(String db) {
      metadata.getDatabasesTouched().add(db);
      return this;
    }

    public Builder expectedStatus(String status) {
      metadata.setExpectedStatus(status);
      return this;
    }

    public Builder expectedErrorMessage(String message) {
      metadata.setExpectedErrorMessage(message);
      return this;
    }

    public Builder expectedError(String message) {
      metadata.setExpectedStatus(STATUS_VALIDATION_FAILED);
      metadata.setExpectedErrorMessage(message);
      return this;
    }

    public Builder expectedRevert(String message) {
      metadata.setExpectedStatus(STATUS_REVERT);
      metadata.setExpectedErrorMessage(message);
      return this;
    }

    public Builder ownerAddress(String address) {
      metadata.setOwnerAddress(address);
      return this;
    }

    public Builder dynamicProperty(String key, Object value) {
      metadata.getDynamicProperties().put(key, value);
      return this;
    }

    public Builder note(String note) {
      metadata.getNotes().add(note);
      return this;
    }

    public FixtureMetadata build() {
      if (isBlank(metadata.getContractType())) {
        throw new IllegalStateException("Contract type is required");
      }
      if (isBlank(metadata.getCaseName())) {
        throw new IllegalStateException("Case name is required");
      }
      if (isBlank(metadata.getCaseCategory())) {
        throw new IllegalStateException("Case category is required");
      }
      if (!isValidCaseCategory(metadata.getCaseCategory())) {
        throw new IllegalStateException(
            "Case category must be one of: happy, validate_fail, edge");
      }
      if (isBlank(metadata.getExpectedStatus())) {
        throw new IllegalStateException("Expected status is required");
      }
      if (!isValidExpectedStatus(metadata.getExpectedStatus())) {
        throw new IllegalStateException(
            "Expected status must be one of: SUCCESS, REVERT, VALIDATION_FAILED");
      }
      if (metadata.getDatabasesTouched() == null || metadata.getDatabasesTouched().isEmpty()) {
        throw new IllegalStateException("At least one database must be specified");
      }
      return metadata;
    }

    private static boolean isBlank(String value) {
      return value == null || value.trim().isEmpty();
    }

    private static boolean isValidCaseCategory(String value) {
      if (value == null) {
        return false;
      }
      switch (value) {
        case "happy":
        case "validate_fail":
        case "edge":
          return true;
        default:
          return false;
      }
    }

    private static boolean isValidExpectedStatus(String value) {
      if (value == null) {
        return false;
      }
      switch (value) {
        case STATUS_SUCCESS:
        case STATUS_REVERT:
        case STATUS_VALIDATION_FAILED:
        case STATUS_OUT_OF_ENERGY:
          return true;
        default:
          return false;
      }
    }
  }
}

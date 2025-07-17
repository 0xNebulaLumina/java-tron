package org.tron.core.execution.spi;

import org.tron.core.capsule.TransactionCapsule;

/**
 * Golden Vector data structure for deterministic testing.
 * 
 * A golden vector represents a specific transaction scenario with expected results
 * that can be used to verify the equivalence between different execution engines.
 */
public class GoldenVector {
  
  private final String name;
  private final String category;
  private final TransactionCapsule transaction;
  private final boolean isContractCall;
  private final ExpectedResult expectedResult;
  private final String description;
  
  public GoldenVector(String name, String category, TransactionCapsule transaction, 
                     boolean isContractCall, ExpectedResult expectedResult) {
    this(name, category, transaction, isContractCall, expectedResult, "");
  }
  
  public GoldenVector(String name, String category, TransactionCapsule transaction, 
                     boolean isContractCall, ExpectedResult expectedResult, String description) {
    this.name = name;
    this.category = category;
    this.transaction = transaction;
    this.isContractCall = isContractCall;
    this.expectedResult = expectedResult;
    this.description = description;
  }
  
  public String getName() {
    return name;
  }
  
  public String getCategory() {
    return category;
  }
  
  public TransactionCapsule getTransaction() {
    return transaction;
  }
  
  public boolean isContractCall() {
    return isContractCall;
  }
  
  public ExpectedResult getExpectedResult() {
    return expectedResult;
  }
  
  public String getDescription() {
    return description;
  }
  
  @Override
  public String toString() {
    return String.format("GoldenVector{name='%s', category='%s', contractCall=%s, description='%s'}", 
                        name, category, isContractCall, description);
  }
  
  /**
   * Expected result for a golden vector test.
   */
  public static class ExpectedResult {
    private final boolean success;
    private final long energyUsed;
    private final byte[] returnData;
    private final String errorMessage;
    private final int stateChangesCount;
    private final long bandwidthUsed;
    
    public ExpectedResult(boolean success, long energyUsed, byte[] returnData, 
                         String errorMessage, int stateChangesCount) {
      this(success, energyUsed, returnData, errorMessage, stateChangesCount, 0);
    }
    
    public ExpectedResult(boolean success, long energyUsed, byte[] returnData, 
                         String errorMessage, int stateChangesCount, long bandwidthUsed) {
      this.success = success;
      this.energyUsed = energyUsed;
      this.returnData = returnData;
      this.errorMessage = errorMessage;
      this.stateChangesCount = stateChangesCount;
      this.bandwidthUsed = bandwidthUsed;
    }
    
    public boolean isSuccess() {
      return success;
    }
    
    public long getEnergyUsed() {
      return energyUsed;
    }
    
    public byte[] getReturnData() {
      return returnData;
    }
    
    public String getErrorMessage() {
      return errorMessage;
    }
    
    public int getStateChangesCount() {
      return stateChangesCount;
    }
    
    public long getBandwidthUsed() {
      return bandwidthUsed;
    }
    
    @Override
    public String toString() {
      return String.format("ExpectedResult{success=%s, energy=%d, bandwidth=%d, stateChanges=%d, error='%s'}", 
                          success, energyUsed, bandwidthUsed, stateChangesCount, errorMessage);
    }
  }
  
  /**
   * Builder for creating golden vectors with fluent API.
   */
  public static class Builder {
    private String name;
    private String category;
    private TransactionCapsule transaction;
    private boolean isContractCall = false;
    private String description = "";
    
    // Expected result fields
    private boolean expectedSuccess = true;
    private long expectedEnergyUsed = 0;
    private byte[] expectedReturnData = null;
    private String expectedErrorMessage = null;
    private int expectedStateChangesCount = -1; // -1 means don't check
    private long expectedBandwidthUsed = 0;
    
    public Builder name(String name) {
      this.name = name;
      return this;
    }
    
    public Builder category(String category) {
      this.category = category;
      return this;
    }
    
    public Builder transaction(TransactionCapsule transaction) {
      this.transaction = transaction;
      return this;
    }
    
    public Builder contractCall(boolean isContractCall) {
      this.isContractCall = isContractCall;
      return this;
    }
    
    public Builder description(String description) {
      this.description = description;
      return this;
    }
    
    public Builder expectSuccess(boolean success) {
      this.expectedSuccess = success;
      return this;
    }
    
    public Builder expectEnergyUsed(long energyUsed) {
      this.expectedEnergyUsed = energyUsed;
      return this;
    }
    
    public Builder expectReturnData(byte[] returnData) {
      this.expectedReturnData = returnData;
      return this;
    }
    
    public Builder expectErrorMessage(String errorMessage) {
      this.expectedErrorMessage = errorMessage;
      return this;
    }
    
    public Builder expectStateChangesCount(int stateChangesCount) {
      this.expectedStateChangesCount = stateChangesCount;
      return this;
    }
    
    public Builder expectBandwidthUsed(long bandwidthUsed) {
      this.expectedBandwidthUsed = bandwidthUsed;
      return this;
    }
    
    public GoldenVector build() {
      if (name == null || category == null || transaction == null) {
        throw new IllegalStateException("Name, category, and transaction are required");
      }
      
      ExpectedResult expectedResult = new ExpectedResult(
          expectedSuccess, expectedEnergyUsed, expectedReturnData, 
          expectedErrorMessage, expectedStateChangesCount, expectedBandwidthUsed);
      
      return new GoldenVector(name, category, transaction, isContractCall, expectedResult, description);
    }
  }
  
  /**
   * Create a new builder for golden vectors.
   */
  public static Builder builder() {
    return new Builder();
  }
  
  /**
   * Common categories for golden vectors.
   */
  public static class Categories {
    public static final String TRANSFER = "TRANSFER";
    public static final String SMART_CONTRACT = "SMART_CONTRACT";
    public static final String EDGE_CASE = "EDGE_CASE";
    public static final String RESOURCE_MANAGEMENT = "RESOURCE_MANAGEMENT";
    public static final String MULTI_SIGNATURE = "MULTI_SIGNATURE";
    public static final String ASSET_MANAGEMENT = "ASSET_MANAGEMENT";
    public static final String GOVERNANCE = "GOVERNANCE";
    public static final String SYSTEM = "SYSTEM";
  }
}

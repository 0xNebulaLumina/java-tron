package org.tron.core.execution.reporting;

import java.util.ArrayList;
import java.util.List;
import org.tron.core.db.TransactionTrace;
import org.tron.common.runtime.ProgramResult;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.core.execution.spi.ExecutionProgramResult;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;
import org.tron.core.execution.spi.ExecutionSpiFactory;
import org.tron.core.storage.spi.StorageSpiFactory;
import org.tron.protos.Protocol.Transaction.Contract;

/**
 * Builder helper for creating ExecutionCsvRecord from transaction execution data.
 * 
 * <p>This class extracts relevant fields from TransactionCapsule, BlockCapsule, 
 * TransactionContext, and ProgramResult to create comprehensive CSV records.
 */
public class ExecutionCsvRecordBuilder {
  
  /**
   * Build CSV record from transaction execution context.
   * 
   * @param trxCap Transaction capsule
   * @param blockCap Block capsule (may be null for pending transactions)
   * @param trace Transaction trace containing execution results
   * @return Complete ExecutionCsvRecord
   */
  public static ExecutionCsvRecord buildRecord(
      TransactionCapsule trxCap, 
      BlockCapsule blockCap, 
      TransactionTrace trace) {
    
    if (trxCap == null || trace == null) {
      return null;
    }
    
    ExecutionCsvRecord.Builder builder = ExecutionCsvRecord.builder();
    
    // Run metadata
    builder.execMode(getExecutionMode())
           .storageMode(getStorageMode());
    
    // Block information
    if (blockCap != null) {
      builder.blockNum(blockCap.getNum())
             .blockIdHex(blockCap.getBlockId().getBytes())
             .isWitnessSigned(blockCap.hasWitnessSignature())
             .blockTimestamp(blockCap.getTimeStamp());
    } else {
      builder.blockNum(0)
             .blockIdHex("")
             .isWitnessSigned(false)
             .blockTimestamp(System.currentTimeMillis());
    }
    
    // Transaction information
    Contract contract = trxCap.getInstance().getRawData().getContract(0);
    builder.txIdHex(trxCap.getTransactionId().getBytes())
           .ownerAddressHex(trxCap.getOwnerAddress())
           .contractType(contract.getType().name())
           .isConstant(isConstantContract(contract))
           .feeLimit(trxCap.getInstance().getRawData().getFeeLimit());
    
    // Transaction index within block (if available)
    if (blockCap != null) {
      int txIndex = findTransactionIndex(blockCap, trxCap);
      builder.txIndexInBlock(txIndex);
    } else {
      builder.txIndexInBlock(-1);
    }
    
    // Extract execution results
    extractExecutionResults(builder, trace);
    
    return builder.build();
  }
  
  /**
   * Extract execution results from transaction trace.
   */
  private static void extractExecutionResults(ExecutionCsvRecord.Builder builder, TransactionTrace trace) {
    TransactionContext context = trace.getTransactionContext();
    if (context == null) {
      // Fallback to receipt if context not available
      extractFromReceipt(builder, trace);
      return;
    }
    
    ProgramResult programResult = context.getProgramResult();
    if (programResult == null) {
      // No execution result available
      builder.isSuccess(false)
             .resultCode("NO_RESULT")
             .energyUsed(0)
             .returnDataHex("")
             .returnDataLen(0)
             .runtimeError("")
             .stateChangeCount(0)
             .stateChanges(new ArrayList<>())
             .stateDigestSha256(StateChangeCanonicalizer.computeEmptyStateDigest());
      return;
    }
    
    // Basic execution results
    builder.isSuccess(isExecutionSuccess(programResult))
           .resultCode(programResult.getResultCode() != null ? programResult.getResultCode().name() : "UNKNOWN")
           .energyUsed(programResult.getEnergyUsed())
           .returnDataHex(programResult.getHReturn())
           .runtimeError(programResult.getRuntimeError() != null ? programResult.getRuntimeError() : "");
    
    // State changes (if available from ExecutionProgramResult)
    if (programResult instanceof ExecutionProgramResult) {
      ExecutionProgramResult execResult = (ExecutionProgramResult) programResult;
      List<StateChange> stateChanges = execResult.getStateChanges();
      
      if (stateChanges != null && !stateChanges.isEmpty()) {
        builder.stateChangeCount(stateChanges.size())
               .stateChanges(stateChanges)
               .stateDigestSha256(StateChangeCanonicalizer.computeStateDigest(stateChanges));
      } else {
        builder.stateChangeCount(0)
               .stateChanges(new ArrayList<>())
               .stateDigestSha256(StateChangeCanonicalizer.computeEmptyStateDigest());
      }
    } else {
      // For embedded execution, get state changes from journal (Phase 2)
      List<StateChange> journaledChanges = StateChangeJournalRegistry.finalizeForCurrentTransaction();
      
      if (journaledChanges != null && !journaledChanges.isEmpty()) {
        builder.stateChangeCount(journaledChanges.size())
               .stateChanges(journaledChanges)
               .stateDigestSha256(StateChangeCanonicalizer.computeStateDigest(journaledChanges));
      } else {
        builder.stateChangeCount(0)
               .stateChanges(new ArrayList<>())
               .stateDigestSha256(StateChangeCanonicalizer.computeEmptyStateDigest());
      }
    }
  }
  
  /**
   * Fallback extraction from receipt when context is not available.
   */
  private static void extractFromReceipt(ExecutionCsvRecord.Builder builder, TransactionTrace trace) {
    // Try to get basic info from receipt
    builder.isSuccess(false)
           .resultCode("NO_CONTEXT")
           .energyUsed(0)
           .returnDataHex("")
           .returnDataLen(0)
           .runtimeError("")
           .stateChangeCount(0)
           .stateChanges(new ArrayList<>())
           .stateDigestSha256(StateChangeCanonicalizer.computeEmptyStateDigest());
  }
  
  /**
   * Determine if execution was successful.
   */
  private static boolean isExecutionSuccess(ProgramResult result) {
    if (result == null) {
      return false;
    }
    
    // Check if there were any exceptions or errors
    if (result.getException() != null) {
      return false;
    }
    
    if (result.isRevert()) {
      return false;
    }
    
    if (result.getRuntimeError() != null && !result.getRuntimeError().isEmpty()) {
      return false;
    }
    
    // Check result code if available
    if (result.getResultCode() != null) {
      return result.getResultCode() == org.tron.protos.Protocol.Transaction.Result.contractResult.SUCCESS;
    }
    
    return true;
  }
  
  /**
   * Check if contract is a constant/view contract.
   */
  private static boolean isConstantContract(Contract contract) {
    // For now, assume all contracts can modify state
    // This can be enhanced later by checking contract ABI
    return false;
  }
  
  /**
   * Find transaction index within block.
   */
  private static int findTransactionIndex(BlockCapsule blockCap, TransactionCapsule trxCap) {
    if (blockCap == null || trxCap == null) {
      return -1;
    }
    
    List<TransactionCapsule> transactions = blockCap.getTransactions();
    for (int i = 0; i < transactions.size(); i++) {
      if (transactions.get(i).getTransactionId().equals(trxCap.getTransactionId())) {
        return i;
      }
    }
    
    return -1;
  }
  
  /**
   * Get current execution mode.
   */
  private static String getExecutionMode() {
    try {
      return ExecutionSpiFactory.determineExecutionMode().toString();
    } catch (Exception e) {
      return "UNKNOWN";
    }
  }
  
  /**
   * Get current storage mode.
   */
  private static String getStorageMode() {
    try {
      return StorageSpiFactory.determineStorageMode().toString();
    } catch (Exception e) {
      return "UNKNOWN";
    }
  }
}
package org.tron.core.execution.spi;

import java.util.ArrayList;
import java.util.List;
import java.util.stream.Collectors;
import lombok.Getter;
import lombok.Setter;
import org.tron.common.runtime.ProgramResult;
import org.tron.common.runtime.vm.DataWord;
import org.tron.common.runtime.vm.LogInfo;
import org.tron.core.execution.reporting.StateChangeJournalRegistry;
import org.tron.core.execution.spi.ExecutionSPI.ExecutionResult;
import org.tron.core.execution.spi.ExecutionSPI.LogEntry;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;
import org.tron.protos.Protocol.Transaction.Result.contractResult;

/**
 * ExecutionProgramResult extends ProgramResult to provide compatibility between ExecutionSPI and
 * RuntimeImpl. This class contains ALL fields from ProgramResult plus additional ExecutionSPI-specific
 * fields, ensuring seamless conversion and backward compatibility.
 *
 * <p>This wrapper allows ExecutionSPI to return a ProgramResult-compatible type while preserving
 * all ExecutionSPI functionality including shadow verification.
 */
public class ExecutionProgramResult extends ProgramResult {

  // Additional ExecutionSPI-specific fields
  @Getter @Setter private List<StateChange> stateChanges;
  @Getter @Setter private long bandwidthUsed;
  // Phase 2: Freeze/resource ledger changes for Java-side application
  @Getter @Setter private List<ExecutionSPI.FreezeLedgerChange> freezeChanges;
  @Getter @Setter private List<ExecutionSPI.GlobalResourceTotalsChange> globalResourceChanges;
  // Phase 2: TRC-10 semantic changes for Java-side application
  @Getter @Setter private List<ExecutionSPI.Trc10Change> trc10Changes;

  /** Default constructor creates an empty result. */
  public ExecutionProgramResult() {
    super();
    this.stateChanges = new ArrayList<>();
    this.bandwidthUsed = 0;
    this.freezeChanges = new ArrayList<>();
    this.globalResourceChanges = new ArrayList<>();
    this.trc10Changes = new ArrayList<>();
  }

  /**
   * Create ExecutionProgramResult from an existing ProgramResult.
   * This preserves all ProgramResult data and adds ExecutionSPI-specific fields.
   *
   * @param programResult The source ProgramResult
   * @return ExecutionProgramResult with all ProgramResult data preserved
   */
  public static ExecutionProgramResult fromProgramResult(ProgramResult programResult) {
    if (programResult == null) {
      return new ExecutionProgramResult();
    }

    ExecutionProgramResult result = new ExecutionProgramResult();

    // Copy all ProgramResult fields
    result.spendEnergy(programResult.getEnergyUsed());
    result.addTotalPenalty(programResult.getEnergyPenaltyTotal());
    result.setHReturn(programResult.getHReturn());
    result.setContractAddress(programResult.getContractAddress());
    result.setException(programResult.getException());
    if (programResult.isRevert()) {
      result.setRevert();
    }
    result.getDeleteAccounts().addAll(programResult.getDeleteAccounts());
    result.addInternalTransactions(programResult.getInternalTransactions());
    result.addLogInfos(programResult.getLogInfoList());
    result.setRet(programResult.getRet());
    result.setTriggerList(programResult.getTriggerList());
    result.setRuntimeError(programResult.getRuntimeError());
    result.setResultCode(programResult.getResultCode());
    result.getCallCreateList().addAll(programResult.getCallCreateList());

    // Initialize ExecutionSPI-specific fields with defaults
    result.bandwidthUsed = 0; // TODO: Calculate from ProgramResult if possible
    
    // Try to get state changes from the current transaction's journal
    try {
      List<StateChange> journaledStateChanges = StateChangeJournalRegistry.getCurrentTransactionStateChanges();
      result.stateChanges = journaledStateChanges != null ? new ArrayList<>(journaledStateChanges) : new ArrayList<>();
    } catch (Exception e) {
      // If journal access fails, use empty list (maintains backwards compatibility)
      result.stateChanges = new ArrayList<>();
    }

    return result;
  }

  /**
   * Create ExecutionProgramResult from ExecutionSPI.ExecutionResult.
   * This converts ExecutionResult data to ProgramResult format.
   *
   * @param executionResult The source ExecutionResult
   * @return ExecutionProgramResult with converted data
   */
  public static ExecutionProgramResult fromExecutionResult(ExecutionResult executionResult) {
    if (executionResult == null) {
      return new ExecutionProgramResult();
    }

    ExecutionProgramResult result = new ExecutionProgramResult();

    // Convert ExecutionResult fields to ProgramResult format
    result.spendEnergy(executionResult.getEnergyUsed());
    result.setHReturn(executionResult.getReturnData());

    // Set success/failure state
    if (executionResult.isSuccess()) {
      result.setResultCode(contractResult.SUCCESS);
    } else {
      result.setResultCode(contractResult.REVERT);
      result.setRevert();
      result.setRuntimeError(executionResult.getErrorMessage());
    }

    // Convert logs from LogEntry to LogInfo
    if (executionResult.getLogs() != null) {
      for (LogEntry logEntry : executionResult.getLogs()) {
        // Convert LogEntry to LogInfo
        LogInfo logInfo = convertLogEntryToLogInfo(logEntry);
        result.addLogInfo(logInfo);
      }
    }

    // Set ExecutionSPI-specific fields
    result.stateChanges = executionResult.getStateChanges() != null
        ? new ArrayList<>(executionResult.getStateChanges())
        : new ArrayList<>();
    result.bandwidthUsed = executionResult.getBandwidthUsed();
    result.freezeChanges = executionResult.getFreezeChanges() != null
        ? new ArrayList<>(executionResult.getFreezeChanges())
        : new ArrayList<>();
    result.globalResourceChanges = executionResult.getGlobalResourceChanges() != null
        ? new ArrayList<>(executionResult.getGlobalResourceChanges())
        : new ArrayList<>();
    result.trc10Changes = executionResult.getTrc10Changes() != null
        ? new ArrayList<>(executionResult.getTrc10Changes())
        : new ArrayList<>();

    return result;
  }

  /**
   * Convert this ExecutionProgramResult to a standard ProgramResult.
   * Since this class extends ProgramResult, this simply returns itself cast to ProgramResult.
   *
   * @return This instance as ProgramResult
   */
  public ProgramResult toProgramResult() {
    return this;
  }

  /**
   * Convert this ExecutionProgramResult to ExecutionSPI.ExecutionResult.
   * This creates a new ExecutionResult with data from this ProgramResult.
   *
   * @return ExecutionResult with converted data
   */
  public ExecutionResult toExecutionResult() {
    // Determine success based on ProgramResult state
    boolean success = getException() == null && !isRevert() && getRuntimeError() == null;

    // Convert LogInfo back to LogEntry
    List<LogEntry> logs = getLogInfoList().stream()
        .map(this::convertLogInfoToLogEntry)
        .collect(Collectors.toList());

    return new ExecutionResult(
        success,
        getHReturn(),
        getEnergyUsed(),
        0, // energyRefunded - TODO: Calculate if needed
        stateChanges != null ? new ArrayList<>(stateChanges) : new ArrayList<>(),
        logs,
        success ? null : (getRuntimeError() != null ? getRuntimeError() : "Execution failed"),
        bandwidthUsed,
        freezeChanges != null ? new ArrayList<>(freezeChanges) : new ArrayList<>(),
        globalResourceChanges != null ? new ArrayList<>(globalResourceChanges) : new ArrayList<>(),
        new ArrayList<>() // trc10Changes - not applicable for VM execution
    );
  }

  /**
   * Check if execution was successful based on ProgramResult state.
   * This provides ExecutionResult-compatible success checking.
   *
   * @return true if execution was successful
   */
  public boolean isSuccess() {
    return getException() == null && !isRevert() && getRuntimeError() == null;
  }

  /**
   * Get error message for failed executions.
   * This provides ExecutionResult-compatible error reporting.
   *
   * @return Error message or null if successful
   */
  public String getErrorMessage() {
    if (isSuccess()) {
      return null;
    }
    if (getRuntimeError() != null) {
      return getRuntimeError();
    }
    if (getException() != null) {
      return getException().getMessage();
    }
    if (isRevert()) {
      return "Transaction reverted";
    }
    return "Unknown execution error";
  }

  // Helper methods for log conversion

  /**
   * Convert ExecutionSPI.LogEntry to LogInfo.
   * LogEntry has: address (byte[]), topics (List<byte[]>), data (byte[])
   * LogInfo has: address (byte[]), topics (List<DataWord>), data (byte[])
   */
  private static LogInfo convertLogEntryToLogInfo(LogEntry logEntry) {
    if (logEntry == null) {
      return new LogInfo(new byte[0], new ArrayList<>(), new byte[0]);
    }

    // Convert byte[] topics to DataWord topics
    List<DataWord> topics = new ArrayList<>();
    if (logEntry.getTopics() != null) {
      for (byte[] topic : logEntry.getTopics()) {
        topics.add(new DataWord(topic));
      }
    }

    return new LogInfo(
        logEntry.getAddress() != null ? logEntry.getAddress() : new byte[0],
        topics,
        logEntry.getData() != null ? logEntry.getData() : new byte[0]
    );
  }

  /**
   * Convert LogInfo to ExecutionSPI.LogEntry.
   * LogInfo has: address (byte[]), topics (List<DataWord>), data (byte[])
   * LogEntry has: address (byte[]), topics (List<byte[]>), data (byte[])
   */
  private LogEntry convertLogInfoToLogEntry(LogInfo logInfo) {
    if (logInfo == null) {
      return new LogEntry(new byte[0], new ArrayList<>(), new byte[0]);
    }

    // Convert DataWord topics to byte[] topics
    List<byte[]> topics = new ArrayList<>();
    if (logInfo.getTopics() != null) {
      for (DataWord topic : logInfo.getTopics()) {
        topics.add(topic.getData());
      }
    }

    return new LogEntry(
        logInfo.getAddress() != null ? logInfo.getAddress() : new byte[0],
        topics,
        logInfo.getData() != null ? logInfo.getData() : new byte[0]
    );
  }
}

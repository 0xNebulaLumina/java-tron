package org.tron.core.execution.spi;

import java.time.Instant;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.stream.Collectors;
import lombok.Data;
import org.tron.common.utils.ByteArray;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.protos.Protocol.Transaction.Contract.ContractType;

/**
 * ExecutionMetrics captures all relevant execution data for tracking and comparison between
 * different execution modes (EMBEDDED, REMOTE, SHADOW).
 *
 * <p>This class encapsulates:
 * - ExecutionProgramResult details (success, energy, return data, errors, state changes)
 * - TransactionContext information (block info, transaction details)
 * - State digest for consistency verification
 * - Execution metadata (timestamps, mode, etc.)
 */
@Data
public class ExecutionMetrics {

  // Execution metadata
  private final Instant timestamp;
  private final String executionMode;
  private final String transactionId;

  // ExecutionProgramResult fields
  private final boolean isSuccess;
  private final long energyUsed;
  private final String returnDataHex;
  private final String runtimeError;
  private final int stateChangesCount;
  private final List<String> stateChangeDetails;

  // TransactionContext fields
  private final long blockNumber;
  private final long blockTimestamp;
  private final String contractType;

  // State digest for consistency verification
  private final String stateDigest;

  // Performance metrics
  private final long executionTimeMs;

  /**
   * Create ExecutionMetrics from execution result and context.
   *
   * @param executionMode The execution mode (EMBEDDED, REMOTE, SHADOW)
   * @param result The execution program result
   * @param context The transaction context
   * @param stateDigest Optional state digest for verification
   * @param executionTimeMs Execution time in milliseconds
   * @return ExecutionMetrics instance
   */
  public static ExecutionMetrics create(
      String executionMode,
      ExecutionProgramResult result,
      TransactionContext context,
      String stateDigest,
      long executionTimeMs) {

    return new ExecutionMetrics(
        Instant.now(),
        executionMode,
        extractTransactionId(context),
        result.isSuccess(),
        result.getEnergyUsed(),
        formatReturnData(result.getHReturn()),
        result.getRuntimeError(),
        extractStateChangesCount(result),
        extractStateChangeDetails(result),
        extractBlockNumber(context),
        extractBlockTimestamp(context),
        extractContractType(context),
        stateDigest,
        executionTimeMs
    );
  }

  /**
   * Create ExecutionMetrics with error information.
   *
   * @param executionMode The execution mode
   * @param context The transaction context
   * @param error The error that occurred
   * @param executionTimeMs Execution time in milliseconds
   * @return ExecutionMetrics instance for failed execution
   */
  public static ExecutionMetrics createError(
      String executionMode,
      TransactionContext context,
      String error,
      long executionTimeMs) {

    return new ExecutionMetrics(
        Instant.now(),
        executionMode,
        extractTransactionId(context),
        false,
        0L,
        "",
        error,
        0,
        Collections.emptyList(),
        extractBlockNumber(context),
        extractBlockTimestamp(context),
        extractContractType(context),
        "",
        executionTimeMs
    );
  }

  /**
   * Format this metrics instance as a CSV row.
   *
   * @return CSV formatted string
   */
  public String toCsvRow() {
    return String.join(",",
        timestamp.toString(),
        escapeForCsv(transactionId),
        escapeForCsv(executionMode),
        Boolean.toString(isSuccess),
        Long.toString(energyUsed),
        escapeForCsv(returnDataHex),
        escapeForCsv(runtimeError != null ? runtimeError : ""),
        Integer.toString(stateChangesCount),
        Long.toString(blockNumber),
        Long.toString(blockTimestamp),
        escapeForCsv(contractType),
        escapeForCsv(stateDigest != null ? stateDigest : ""),
        Long.toString(executionTimeMs)
    );
  }

  /**
   * Get CSV header row.
   *
   * @return CSV header string
   */
  public static String getCsvHeader() {
    return "timestamp,tx_id,execution_mode,is_success,energy_used,return_data_hex,runtime_error,"
        + "state_changes_count,block_number,block_timestamp,tx_type,state_digest,execution_time_ms";
  }

  // Helper methods for data extraction

  private static String extractTransactionId(TransactionContext context) {
    try {
      if (context != null && context.getTrxCap() != null) {
        return context.getTrxCap().getTransactionId().toString();
      }
    } catch (Exception e) {
      // Ignore extraction errors
    }
    return "unknown";
  }

  private static String formatReturnData(byte[] returnData) {
    if (returnData == null || returnData.length == 0) {
      return "";
    }
    return ByteArray.toHexString(returnData);
  }

  private static int extractStateChangesCount(ExecutionProgramResult result) {
    try {
      if (result != null && result.getStateChanges() != null) {
        return result.getStateChanges().size();
      }
    } catch (Exception e) {
      // Ignore extraction errors
    }
    return 0;
  }

  private static List<String> extractStateChangeDetails(ExecutionProgramResult result) {
    try {
      if (result != null && result.getStateChanges() != null) {
        return result.getStateChanges().stream()
            .map(change -> String.format("%s:%s->%s",
                ByteArray.toHexString(change.getAddress()),
                ByteArray.toHexString(change.getKey()),
                ByteArray.toHexString(change.getNewValue())))
            .collect(Collectors.toList());
      }
    } catch (Exception e) {
      // Ignore extraction errors
    }
    return Collections.emptyList();
  }

  private static long extractBlockNumber(TransactionContext context) {
    try {
      if (context != null && context.getBlockCap() != null) {
        return context.getBlockCap().getNum();
      }
    } catch (Exception e) {
      // Ignore extraction errors
    }
    return -1L;
  }

  private static long extractBlockTimestamp(TransactionContext context) {
    try {
      if (context != null && context.getBlockCap() != null) {
        return context.getBlockCap().getTimeStamp();
      }
    } catch (Exception e) {
      // Ignore extraction errors
    }
    return -1L;
  }

  private static String extractContractType(TransactionContext context) {
    try {
      if (context != null && context.getTrxCap() != null) {
        TransactionCapsule trxCap = context.getTrxCap();
        if (trxCap.getInstance().getRawData().getContractCount() > 0) {
          ContractType type = trxCap.getInstance().getRawData().getContract(0).getType();
          return type.name();
        }
      }
    } catch (Exception e) {
      // Ignore extraction errors
    }
    return "unknown";
  }

  private static String escapeForCsv(String value) {
    if (value == null) {
      return "";
    }
    // Escape quotes and wrap in quotes if contains comma, quote, or newline
    if (value.contains(",") || value.contains("\"") || value.contains("\n")) {
      return "\"" + value.replace("\"", "\"\"") + "\"";
    }
    return value;
  }
}
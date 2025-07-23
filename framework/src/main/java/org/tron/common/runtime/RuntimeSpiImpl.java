package org.tron.common.runtime;

import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import lombok.extern.slf4j.Slf4j;
import org.tron.common.runtime.vm.DataWord;
import org.tron.common.runtime.vm.LogInfo;
import org.tron.core.db.TransactionContext;
import org.tron.core.exception.ContractExeException;
import org.tron.core.exception.ContractValidateException;

import org.tron.core.execution.spi.ExecutionSPI;
import org.tron.core.execution.spi.ExecutionSpiFactory;
import org.tron.protos.Protocol.Transaction.Result.contractResult;

/**
 * ExecutionSPI-aware Runtime implementation that maintains the existing Runtime interface
 * while delegating execution to the configured ExecutionSPI implementation (EMBEDDED, REMOTE, or SHADOW).
 * 
 * This class provides backward compatibility by converting between ExecutionSPI.ExecutionResult
 * and the existing ProgramResult format.
 */
@Slf4j(topic = "VM")
public class RuntimeSpiImpl implements Runtime {
    
    private final ExecutionSPI executionSPI;
    private TransactionContext context;
    private ExecutionSPI.ExecutionResult executionResult;
    private String runtimeError;
    
    public RuntimeSpiImpl() {
        this.executionSPI = ExecutionSpiFactory.getInstance();
        if (this.executionSPI == null) {
            throw new RuntimeException("ExecutionSPI not initialized. Call ExecutionSpiFactory.initialize() first.");
        }
        logger.info("RuntimeSpiImpl initialized with execution mode: {}", 
                   ExecutionSpiFactory.determineExecutionMode());
    }
    
    @Override
    public void execute(TransactionContext context) 
            throws ContractValidateException, ContractExeException {
        this.context = context;
        
        try {
            logger.debug("Executing transaction with ExecutionSPI: {}", 
                        context.getTrxCap().getTransactionId());
            
            // Use ExecutionSPI for execution
            CompletableFuture<ExecutionSPI.ExecutionResult> future = 
                executionSPI.executeTransaction(context);
            this.executionResult = future.get(); // Synchronous execution
            
            // Store runtime error if execution failed
            if (!executionResult.isSuccess()) {
                this.runtimeError = executionResult.getErrorMessage();
            }
            
            // Convert ExecutionResult back to ProgramResult for compatibility
            convertExecutionResultToProgramResult();
            
            logger.debug("ExecutionSPI execution completed. Success: {}, Energy used: {}", 
                        executionResult.isSuccess(), executionResult.getEnergyUsed());
            
        } catch (Exception e) {
            logger.error("ExecutionSPI execution failed for transaction: {}", 
                        context.getTrxCap().getTransactionId(), e);
            
            // Create a failed ProgramResult for compatibility
            createFailedProgramResult(e.getMessage());
            this.runtimeError = e.getMessage();
            
            throw new ContractExeException("Execution failed: " + e.getMessage());
        }
    }
    
    @Override
    public ProgramResult getResult() {
        if (context == null) {
            return ProgramResult.createEmpty();
        }
        return context.getProgramResult();
    }
    
    @Override
    public String getRuntimeError() {
        return runtimeError;
    }
    
    /**
     * Convert ExecutionSPI.ExecutionResult to ProgramResult for backward compatibility.
     * This method populates the TransactionContext's ProgramResult with data from ExecutionResult.
     */
    private void convertExecutionResultToProgramResult() {
        if (executionResult == null || context == null) {
            return;
        }
        
        ProgramResult programResult = context.getProgramResult();
        if (programResult == null) {
            programResult = new ProgramResult();
            context.setProgramResult(programResult);
        }
        
        // Set basic execution results
        programResult.spendEnergy(executionResult.getEnergyUsed());
        programResult.setHReturn(executionResult.getReturnData());
        
        // Set result code based on success/failure
        if (executionResult.isSuccess()) {
            programResult.setResultCode(contractResult.SUCCESS);
        } else {
            programResult.setResultCode(contractResult.REVERT);
            programResult.setRevert();
            programResult.setRuntimeError(executionResult.getErrorMessage());
        }
        
        // Convert logs from ExecutionResult to LogInfo
        convertLogsToLogInfo(programResult);
        
        // Handle state changes (if needed for compatibility)
        // Note: State changes are typically handled by the storage layer
        // but we may need to track them for certain compatibility scenarios
        
        logger.debug("Converted ExecutionResult to ProgramResult. Energy: {}, Success: {}", 
                    executionResult.getEnergyUsed(), executionResult.isSuccess());
    }
    
    /**
     * Convert ExecutionSPI logs to ProgramResult LogInfo format.
     */
    private void convertLogsToLogInfo(ProgramResult programResult) {
        if (executionResult.getLogs() == null || executionResult.getLogs().isEmpty()) {
            return;
        }

        List<LogInfo> logInfoList = new ArrayList<>();
        for (ExecutionSPI.LogEntry logEntry : executionResult.getLogs()) {
            // Convert byte[] topics to DataWord topics
            List<DataWord> topics = new ArrayList<>();
            if (logEntry.getTopics() != null) {
                for (byte[] topic : logEntry.getTopics()) {
                    topics.add(new DataWord(topic));
                }
            }

            // Convert ExecutionSPI.LogEntry to LogInfo
            LogInfo logInfo = new LogInfo(
                logEntry.getAddress(),
                topics,
                logEntry.getData()
            );
            logInfoList.add(logInfo);
        }

        programResult.addLogInfos(logInfoList);
        logger.debug("Converted {} logs from ExecutionResult to LogInfo", logInfoList.size());
    }
    
    /**
     * Create a failed ProgramResult when ExecutionSPI execution fails.
     */
    private void createFailedProgramResult(String errorMessage) {
        if (context == null) {
            return;
        }
        
        ProgramResult programResult = context.getProgramResult();
        if (programResult == null) {
            programResult = new ProgramResult();
            context.setProgramResult(programResult);
        }
        
        // Set failure state
        programResult.setResultCode(contractResult.REVERT);
        programResult.setRevert();
        programResult.setRuntimeError(errorMessage);
        programResult.setException(new RuntimeException(errorMessage));
        
        logger.debug("Created failed ProgramResult with error: {}", errorMessage);
    }
}

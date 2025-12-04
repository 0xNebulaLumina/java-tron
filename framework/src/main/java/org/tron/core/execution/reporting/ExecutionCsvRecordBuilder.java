package org.tron.core.execution.reporting;

import java.util.ArrayList;
import java.util.List;
import org.tron.common.runtime.ProgramResult;
import org.tron.common.runtime.vm.LogInfo;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.core.db.TransactionTrace;
import org.tron.core.execution.spi.ExecutionProgramResult;
import org.tron.core.execution.spi.ExecutionSPI.FreezeLedgerChange;
import org.tron.core.execution.spi.ExecutionSPI.GlobalResourceTotalsChange;
import org.tron.core.execution.spi.ExecutionSPI.LogEntry;
import org.tron.core.execution.spi.ExecutionSPI.StateChange;
import org.tron.core.execution.spi.ExecutionSPI.Trc10AssetIssued;
import org.tron.core.execution.spi.ExecutionSPI.Trc10AssetTransferred;
import org.tron.core.execution.spi.ExecutionSPI.Trc10Change;
import org.tron.core.execution.spi.ExecutionSPI.VoteChange;
import org.tron.core.execution.spi.ExecutionSPI.VoteEntry;
import org.tron.core.execution.spi.ExecutionSpiFactory;
import org.tron.core.storage.spi.StorageSpiFactory;
import org.tron.common.utils.ByteArray;
import org.tron.protos.Protocol.Transaction.Contract;

/**
 * Builder helper for creating ExecutionCsvRecord from transaction execution data.
 *
 * <p>This class extracts relevant fields from TransactionCapsule, BlockCapsule,
 * TransactionContext, and ProgramResult to create comprehensive CSV records.
 *
 * <p>Supports domain-specific change extraction for:
 * - Account changes (split from state changes)
 * - EVM storage changes (split from state changes)
 * - TRC-10 balance changes
 * - TRC-10 issuance changes
 * - Vote changes
 * - Freeze changes
 * - Global resource changes
 * - Account resource usage (AEXT) changes
 * - Log entries
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
  private static void extractExecutionResults(
      ExecutionCsvRecord.Builder builder, TransactionTrace trace) {
    TransactionContext context = trace.getTransactionContext();
    if (context == null) {
      // Fallback to receipt if context not available
      extractFromReceipt(builder, trace);
      return;
    }

    ProgramResult programResult = context.getProgramResult();
    if (programResult == null) {
      // No execution result available
      setEmptyExecutionResults(builder);
      return;
    }

    // Basic execution results
    builder.isSuccess(isExecutionSuccess(programResult))
           .resultCode(programResult.getResultCode() != null
               ? programResult.getResultCode().name() : "UNKNOWN")
           .energyUsed(programResult.getEnergyUsed())
           .returnDataHex(programResult.getHReturn())
           .runtimeError(programResult.getRuntimeError() != null
               ? programResult.getRuntimeError() : "");

    // Handle state changes and domain extraction
    if (programResult instanceof ExecutionProgramResult) {
      extractFromExecutionProgramResult(builder, (ExecutionProgramResult) programResult);
    } else {
      // For embedded execution, get state changes from journal
      extractFromEmbeddedExecution(builder, programResult);
    }
  }

  /**
   * Extract domain data from ExecutionProgramResult (remote execution).
   */
  private static void extractFromExecutionProgramResult(
      ExecutionCsvRecord.Builder builder, ExecutionProgramResult execResult) {

    List<StateChange> stateChanges = execResult.getStateChanges();

    // Legacy state changes (aggregate)
    if (stateChanges != null && !stateChanges.isEmpty()) {
      builder.stateChangeCount(stateChanges.size())
             .stateChanges(stateChanges)
             .stateDigestSha256(StateChangeCanonicalizer.computeStateDigest(stateChanges));
    } else {
      builder.stateChangeCount(0)
             .stateChanges(new ArrayList<>())
             .stateDigestSha256(StateChangeCanonicalizer.computeEmptyStateDigest());
    }

    // Split state changes into account and EVM storage domains
    DomainCanonicalizer.SplitStateChanges split =
        DomainCanonicalizer.splitStateChanges(stateChanges);

    // Domain: Account changes
    DomainCanonicalizer.DomainResult accountResult =
        DomainCanonicalizer.accountToJsonAndDigest(split.accountChanges);
    builder.accountDomain(accountResult);

    // Domain: EVM storage changes
    DomainCanonicalizer.DomainResult evmStorageResult =
        DomainCanonicalizer.evmStorageToJsonAndDigest(split.evmStorageChanges);
    builder.evmStorageDomain(evmStorageResult);

    // Domain: TRC-10 changes
    extractTrc10Domains(builder, execResult.getTrc10Changes());

    // Domain: Vote changes
    List<DomainCanonicalizer.VoteDelta> voteDeltas =
        DomainCanonicalizer.convertVoteChanges(execResult.getVoteChanges());
    DomainCanonicalizer.DomainResult voteResult =
        DomainCanonicalizer.votesToJsonAndDigest(voteDeltas);
    builder.voteDomain(voteResult);

    // Domain: Freeze changes
    List<DomainCanonicalizer.FreezeDelta> freezeDeltas =
        DomainCanonicalizer.convertFreezeChanges(execResult.getFreezeChanges());
    DomainCanonicalizer.DomainResult freezeResult =
        DomainCanonicalizer.freezesToJsonAndDigest(freezeDeltas);
    builder.freezeDomain(freezeResult);

    // Domain: Global resource changes
    List<DomainCanonicalizer.GlobalResourceDelta> globalDeltas =
        DomainCanonicalizer.convertGlobalResourceChanges(execResult.getGlobalResourceChanges());
    DomainCanonicalizer.DomainResult globalResult =
        DomainCanonicalizer.globalsToJsonAndDigest(globalDeltas);
    builder.globalResourceDomain(globalResult);

    // Domain: Account resource usage (AEXT) - parsed from account state change bytes
    List<DomainCanonicalizer.AccountResourceUsageDelta> aextDeltas =
        DomainCanonicalizer.extractAccountResourceUsage(stateChanges);
    DomainCanonicalizer.DomainResult aextResult =
        DomainCanonicalizer.accountAextToJsonAndDigest(aextDeltas);
    builder.accountResourceUsageDomain(aextResult);

    // Domain: Log entries
    List<DomainCanonicalizer.LogEntryDelta> logDeltas =
        DomainCanonicalizer.convertLogInfos(execResult.getLogInfoList());
    DomainCanonicalizer.DomainResult logResult =
        DomainCanonicalizer.logsToJsonAndDigest(logDeltas);
    builder.logsDomain(logResult);
  }

  /**
   * Extract TRC-10 balance and issuance domains from Trc10Change list.
   * Uses PreStateSnapshotRegistry for absolute old/new values when available.
   */
  private static void extractTrc10Domains(
      ExecutionCsvRecord.Builder builder, List<Trc10Change> trc10Changes) {

    List<DomainCanonicalizer.Trc10BalanceDelta> balanceDeltas = new ArrayList<>();
    List<DomainCanonicalizer.Trc10IssuanceDelta> issuanceDeltas = new ArrayList<>();

    if (trc10Changes != null) {
      for (Trc10Change change : trc10Changes) {
        if (change.hasAssetTransferred()) {
          // Balance change from transfer
          Trc10AssetTransferred transfer = change.getAssetTransferred();
          String tokenId = transfer.getTokenId() != null && !transfer.getTokenId().isEmpty()
              ? transfer.getTokenId()
              : ByteArray.toHexString(transfer.getAssetName());

          byte[] senderAddr = transfer.getOwnerAddress();
          byte[] recipientAddr = transfer.getToAddress();
          long amount = transfer.getAmount();

          // Get absolute old balances from pre-state snapshot (if available)
          Long senderOldBalance = PreStateSnapshotRegistry.getTrc10Balance(senderAddr, tokenId);
          Long recipientOldBalance = PreStateSnapshotRegistry.getTrc10Balance(recipientAddr, tokenId);

          // Compute absolute new balances
          long senderOld = senderOldBalance != null ? senderOldBalance : 0L;
          long recipientOld = recipientOldBalance != null ? recipientOldBalance : 0L;
          long senderNew = senderOld - amount;
          long recipientNew = recipientOld + amount;

          // Determine op based on old/new values
          String senderOp = (senderOld == 0) ? "delete" : (senderNew == 0) ? "delete" : "update";
          String recipientOp = (recipientOld == 0) ? "create" : "update";

          // Sender decrease
          DomainCanonicalizer.Trc10BalanceDelta senderDelta =
              new DomainCanonicalizer.Trc10BalanceDelta();
          senderDelta.setTokenId(tokenId);
          senderDelta.setOwnerAddressHex(senderAddr != null
              ? ByteArray.toHexString(senderAddr) : "");
          senderDelta.setOp(senderOp);
          senderDelta.setOldBalance(String.valueOf(senderOld));
          senderDelta.setNewBalance(String.valueOf(senderNew));
          balanceDeltas.add(senderDelta);

          // Recipient increase
          DomainCanonicalizer.Trc10BalanceDelta recipientDelta =
              new DomainCanonicalizer.Trc10BalanceDelta();
          recipientDelta.setTokenId(tokenId);
          recipientDelta.setOwnerAddressHex(recipientAddr != null
              ? ByteArray.toHexString(recipientAddr) : "");
          recipientDelta.setOp(recipientOp);
          recipientDelta.setOldBalance(String.valueOf(recipientOld));
          recipientDelta.setNewBalance(String.valueOf(recipientNew));
          balanceDeltas.add(recipientDelta);
        }

        if (change.hasAssetIssued()) {
          // Issuance creates new token metadata
          Trc10AssetIssued issued = change.getAssetIssued();
          String tokenId = issued.getTokenId() != null && !issued.getTokenId().isEmpty()
              ? issued.getTokenId()
              : org.tron.common.utils.ByteArray.toHexString(issued.getName());

          // Create issuance deltas for each field
          addIssuanceDelta(issuanceDeltas, tokenId, "total_supply",
              null, String.valueOf(issued.getTotalSupply()));
          addIssuanceDelta(issuanceDeltas, tokenId, "name",
              null, new String(issued.getName()));
          addIssuanceDelta(issuanceDeltas, tokenId, "abbr",
              null, new String(issued.getAbbr()));
          addIssuanceDelta(issuanceDeltas, tokenId, "precision",
              null, String.valueOf(issued.getPrecision()));
          addIssuanceDelta(issuanceDeltas, tokenId, "trx_num",
              null, String.valueOf(issued.getTrxNum()));
          addIssuanceDelta(issuanceDeltas, tokenId, "num",
              null, String.valueOf(issued.getNum()));
          addIssuanceDelta(issuanceDeltas, tokenId, "start_time",
              null, String.valueOf(issued.getStartTime()));
          addIssuanceDelta(issuanceDeltas, tokenId, "end_time",
              null, String.valueOf(issued.getEndTime()));
          addIssuanceDelta(issuanceDeltas, tokenId, "description",
              null, new String(issued.getDescription()));
          addIssuanceDelta(issuanceDeltas, tokenId, "url",
              null, new String(issued.getUrl()));
          addIssuanceDelta(issuanceDeltas, tokenId, "free_asset_net_limit",
              null, String.valueOf(issued.getFreeAssetNetLimit()));
          addIssuanceDelta(issuanceDeltas, tokenId, "public_free_asset_net_limit",
              null, String.valueOf(issued.getPublicFreeAssetNetLimit()));
        }
      }
    }

    // Domain: TRC-10 balance changes
    DomainCanonicalizer.DomainResult balanceResult =
        DomainCanonicalizer.trc10BalancesToJsonAndDigest(balanceDeltas);
    builder.trc10BalanceDomain(balanceResult);

    // Domain: TRC-10 issuance changes
    DomainCanonicalizer.DomainResult issuanceResult =
        DomainCanonicalizer.trc10IssuanceToJsonAndDigest(issuanceDeltas);
    builder.trc10IssuanceDomain(issuanceResult);
  }

  /**
   * Helper to add TRC-10 issuance delta.
   */
  private static void addIssuanceDelta(
      List<DomainCanonicalizer.Trc10IssuanceDelta> deltas,
      String tokenId, String field, String oldValue, String newValue) {
    DomainCanonicalizer.Trc10IssuanceDelta delta = new DomainCanonicalizer.Trc10IssuanceDelta();
    delta.setTokenId(tokenId);
    delta.setField(field);
    delta.setOp("create");
    delta.setOldValue(oldValue);
    delta.setNewValue(newValue);
    deltas.add(delta);
  }

  /**
   * Extract domain data from embedded execution (using journals).
   */
  private static void extractFromEmbeddedExecution(
      ExecutionCsvRecord.Builder builder, ProgramResult programResult) {

    // Get state changes from journal (don't finalize here, just get current changes)
    List<StateChange> journaledChanges = StateChangeJournalRegistry.getCurrentTransactionStateChanges();

    if (journaledChanges != null && !journaledChanges.isEmpty()) {
      builder.stateChangeCount(journaledChanges.size())
             .stateChanges(journaledChanges)
             .stateDigestSha256(StateChangeCanonicalizer.computeStateDigest(journaledChanges));
    } else {
      builder.stateChangeCount(0)
             .stateChanges(new ArrayList<>())
             .stateDigestSha256(StateChangeCanonicalizer.computeEmptyStateDigest());
    }

    // Split state changes into account and EVM storage domains
    DomainCanonicalizer.SplitStateChanges split =
        DomainCanonicalizer.splitStateChanges(journaledChanges);

    // Domain: Account changes
    DomainCanonicalizer.DomainResult accountResult =
        DomainCanonicalizer.accountToJsonAndDigest(split.accountChanges);
    builder.accountDomain(accountResult);

    // Domain: EVM storage changes
    DomainCanonicalizer.DomainResult evmStorageResult =
        DomainCanonicalizer.evmStorageToJsonAndDigest(split.evmStorageChanges);
    builder.evmStorageDomain(evmStorageResult);

    // Get domain changes from DomainChangeJournalRegistry
    // Domain: TRC-10 balance changes
    List<DomainCanonicalizer.Trc10BalanceDelta> trc10BalanceDeltas =
        DomainChangeJournalRegistry.getTrc10BalanceChanges();
    DomainCanonicalizer.DomainResult trc10BalanceResult =
        DomainCanonicalizer.trc10BalancesToJsonAndDigest(trc10BalanceDeltas);
    builder.trc10BalanceDomain(trc10BalanceResult);

    // Domain: TRC-10 issuance changes
    List<DomainCanonicalizer.Trc10IssuanceDelta> trc10IssuanceDeltas =
        DomainChangeJournalRegistry.getTrc10IssuanceChanges();
    DomainCanonicalizer.DomainResult trc10IssuanceResult =
        DomainCanonicalizer.trc10IssuanceToJsonAndDigest(trc10IssuanceDeltas);
    builder.trc10IssuanceDomain(trc10IssuanceResult);

    // Domain: Vote changes
    List<DomainCanonicalizer.VoteDelta> voteDeltas =
        DomainChangeJournalRegistry.getVoteChanges();
    DomainCanonicalizer.DomainResult voteResult =
        DomainCanonicalizer.votesToJsonAndDigest(voteDeltas);
    builder.voteDomain(voteResult);

    // Domain: Freeze changes
    List<DomainCanonicalizer.FreezeDelta> freezeDeltas =
        DomainChangeJournalRegistry.getFreezeChanges();
    DomainCanonicalizer.DomainResult freezeResult =
        DomainCanonicalizer.freezesToJsonAndDigest(freezeDeltas);
    builder.freezeDomain(freezeResult);

    // Domain: Global resource changes
    List<DomainCanonicalizer.GlobalResourceDelta> globalDeltas =
        DomainChangeJournalRegistry.getGlobalResourceChanges();
    DomainCanonicalizer.DomainResult globalResult =
        DomainCanonicalizer.globalsToJsonAndDigest(globalDeltas);
    builder.globalResourceDomain(globalResult);

    // Domain: Account resource usage (AEXT) - parsed from account state change bytes
    List<DomainCanonicalizer.AccountResourceUsageDelta> aextDeltas =
        DomainCanonicalizer.extractAccountResourceUsage(journaledChanges);
    DomainCanonicalizer.DomainResult aextResult =
        DomainCanonicalizer.accountAextToJsonAndDigest(aextDeltas);
    builder.accountResourceUsageDomain(aextResult);

    // Domain: Log entries from ProgramResult
    List<DomainCanonicalizer.LogEntryDelta> logDeltas =
        DomainCanonicalizer.convertLogInfos(programResult.getLogInfoList());
    DomainCanonicalizer.DomainResult logResult =
        DomainCanonicalizer.logsToJsonAndDigest(logDeltas);
    builder.logsDomain(logResult);
  }

  /**
   * Set empty execution results when no context/result available.
   */
  private static void setEmptyExecutionResults(ExecutionCsvRecord.Builder builder) {
    builder.isSuccess(false)
           .resultCode("NO_RESULT")
           .energyUsed(0)
           .returnDataHex("")
           .returnDataLen(0)
           .runtimeError("")
           .stateChangeCount(0)
           .stateChanges(new ArrayList<>())
           .stateDigestSha256(StateChangeCanonicalizer.computeEmptyStateDigest());

    // All domains empty
    builder.accountDomain(DomainCanonicalizer.emptyDomainResult());
    builder.evmStorageDomain(DomainCanonicalizer.emptyDomainResult());
    builder.trc10BalanceDomain(DomainCanonicalizer.emptyDomainResult());
    builder.trc10IssuanceDomain(DomainCanonicalizer.emptyDomainResult());
    builder.voteDomain(DomainCanonicalizer.emptyDomainResult());
    builder.freezeDomain(DomainCanonicalizer.emptyDomainResult());
    builder.globalResourceDomain(DomainCanonicalizer.emptyDomainResult());
    builder.accountResourceUsageDomain(DomainCanonicalizer.emptyDomainResult());
    builder.logsDomain(DomainCanonicalizer.emptyDomainResult());
  }

  /**
   * Fallback extraction from receipt when context is not available.
   */
  private static void extractFromReceipt(
      ExecutionCsvRecord.Builder builder, TransactionTrace trace) {
    setEmptyExecutionResults(builder);
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
      return result.getResultCode()
          == org.tron.protos.Protocol.Transaction.Result.contractResult.SUCCESS;
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

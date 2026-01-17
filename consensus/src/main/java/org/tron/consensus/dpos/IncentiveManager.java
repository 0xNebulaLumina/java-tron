package org.tron.consensus.dpos;

import static org.tron.core.config.Parameter.ChainConstant.WITNESS_STANDBY_LENGTH;

import com.google.protobuf.ByteString;
import java.util.List;
import lombok.extern.slf4j.Slf4j;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.stereotype.Component;
import org.tron.common.utils.ByteArray;
import org.tron.consensus.ConsensusDelegate;
import org.tron.core.capsule.AccountCapsule;

@Slf4j(topic = "consensus")
@Component
public class IncentiveManager {

  private static final String TRACE_ADDRESS_HEX_PROP = "reward.trace.address_hex";

  @Autowired
  private ConsensusDelegate consensusDelegate;

  private static boolean isTraceAddress(byte[] address) {
    String targetHex = System.getProperty(TRACE_ADDRESS_HEX_PROP);
    if (targetHex == null || targetHex.trim().isEmpty() || address == null) {
      return false;
    }
    return ByteArray.toHexString(address).equalsIgnoreCase(targetHex.trim());
  }

  public void reward(List<ByteString> witnesses) {
    if (consensusDelegate.allowChangeDelegation()) {
      return;
    }
    if (witnesses.size() > WITNESS_STANDBY_LENGTH) {
      witnesses = witnesses.subList(0, WITNESS_STANDBY_LENGTH);
    }
    long voteSum = 0;
    for (ByteString witness : witnesses) {
      voteSum += consensusDelegate.getWitness(witness.toByteArray()).getVoteCount();
    }
    if (voteSum <= 0) {
      return;
    }
    long totalPay = consensusDelegate.getWitnessStandbyAllowance();
    for (int i = 0; i < witnesses.size(); i++) {
      ByteString witness = witnesses.get(i);
      byte[] address = witness.toByteArray();
      long witnessVoteCount = consensusDelegate.getWitness(address).getVoteCount();
      long pay = (long) (witnessVoteCount * ((double) totalPay / voteSum));
      AccountCapsule accountCapsule = consensusDelegate.getAccount(address);
      long allowanceBefore = accountCapsule.getAllowance();
      long allowanceAfter = allowanceBefore + pay;
      accountCapsule.setAllowance(allowanceAfter);
      consensusDelegate.saveAccount(accountCapsule);
      if (isTraceAddress(address)) {
        logger.info(
            "TRACE standby_reward: blk={}, idx={}, addr={}, voteCount={}, voteSum={}, totalPay={}, "
                + "pay={}, allowBefore={}, allowAfter={}",
            consensusDelegate.getLatestBlockHeaderNumber(),
            i,
            ByteArray.toHexString(address),
            witnessVoteCount,
            voteSum,
            totalPay,
            pay,
            allowanceBefore,
            allowanceAfter);
      }
    }
  }
}

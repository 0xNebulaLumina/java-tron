package org.tron.core.execution.reporting;

import org.bouncycastle.crypto.digests.KeccakDigest;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.core.capsule.AccountCapsule;

import static org.tron.protos.contract.Common.ResourceCode.BANDWIDTH;
import static org.tron.protos.contract.Common.ResourceCode.ENERGY;

/**
 * Codec for serializing and deserializing AccountInfo in CSV-compatible format.
 * This codec ensures byte-for-byte parity between embedded and remote execution
 * by providing a canonical serialization format for account state.
 *
 * <p>Format: [balance(32)] + [nonce(8)] + [code_hash(32)] + [code_length(4)] + [code(variable)]
 *           + optional [AEXT tail] for resource usage
 *
 * <p>AEXT tail format (v1):
 * - magic: "AEXT" (4 bytes)
 * - version: 1 (u16 big-endian, 2 bytes)
 * - length: 68 (u16 big-endian, 2 bytes)
 * - payload: resource usage fields (68 bytes)
 *
 * <p>Usage:
 * <pre>
 *   AccountCapsule account = ...;
 *   byte[] serialized = AccountInfoCodec.serialize(account);
 *   // For TRC-10 synthesis:
 *   byte[] serializedWithAext = AccountInfoCodec.serializeWithAext(account, true);
 * </pre>
 */
public class AccountInfoCodec {

  private static final Logger logger = LoggerFactory.getLogger(AccountInfoCodec.class);

  /**
   * System property to control AEXT tail inclusion.
   * Default: true
   */
  public static final String PROPERTY_INCLUDE_AEXT = "remote.exec.accountinfo.resources.enabled";

  /**
   * Serialize AccountCapsule to byte array aligned with remote format.
   * AEXT tail inclusion is controlled by system property (default: true).
   *
   * @param account Account to serialize (null returns empty array)
   * @return Serialized account bytes
   */
  public static byte[] serialize(AccountCapsule account) {
    if (account == null) {
      return new byte[0];
    }

    boolean includeResourceUsage = Boolean.parseBoolean(
        System.getProperty(PROPERTY_INCLUDE_AEXT, "true"));

    return serializeWithAext(account, includeResourceUsage);
  }

  /**
   * Serialize AccountCapsule with explicit AEXT tail control.
   *
   * @param account Account to serialize (null returns empty array)
   * @param includeAext Whether to include AEXT tail
   * @return Serialized account bytes
   */
  public static byte[] serializeWithAext(AccountCapsule account, boolean includeAext) {
    if (account == null) {
      return new byte[0];
    }

    try {
      // Get account data
      long balance = account.getBalance();
      // TRON does not use Ethereum-style nonces for accounts; emit 0
      long nonce = 0L;
      byte[] code = account.getInstance().getCode().toByteArray();
      byte[] codeHash = keccak256(code != null ? code : new byte[0]);

      // Calculate base size: balance(32) + nonce(8) + code_hash(32) + code_len(4) + code
      int codeLength = (code != null ? code.length : 0);
      int baseSize = 32 + 8 + 32 + 4 + codeLength;

      int totalSize = baseSize;
      byte[] aextTail = null;

      if (includeAext) {
        try {
          aextTail = serializeAextTail(account);
          totalSize += aextTail.length;
        } catch (Exception e) {
          logger.warn("Failed to serialize AEXT tail, falling back to base format: {}", e.getMessage());
          // Continue with base format only
        }
      }

      byte[] result = new byte[totalSize];
      int offset = 0;

      // Balance (32 bytes, big-endian)
      byte[] balanceBytes = longToBytes32(balance);
      System.arraycopy(balanceBytes, 0, result, offset, 32);
      offset += 32;

      // Nonce (8 bytes, big-endian)
      byte[] nonceBytes = longToBytes8(nonce);
      System.arraycopy(nonceBytes, 0, result, offset, 8);
      offset += 8;

      // Code hash (32 bytes)
      System.arraycopy(codeHash, 0, result, offset, 32);
      offset += 32;

      // Code length (4 bytes, big-endian)
      result[offset++] = (byte) (codeLength >>> 24);
      result[offset++] = (byte) (codeLength >>> 16);
      result[offset++] = (byte) (codeLength >>> 8);
      result[offset++] = (byte) codeLength;

      // Code (variable length)
      if (code != null && code.length > 0) {
        System.arraycopy(code, 0, result, offset, code.length);
        offset += code.length;
      }

      // Append AEXT tail if present
      if (aextTail != null && aextTail.length > 0) {
        System.arraycopy(aextTail, 0, result, offset, aextTail.length);
        logger.debug("Appended AEXT tail ({} bytes) to account serialization", aextTail.length);
      }

      return result;
    } catch (Exception e) {
      logger.warn("Failed to serialize account info", e);
      return new byte[0];
    }
  }

  /**
   * Serialize AEXT (Account EXTension) v1 tail with resource usage fields.
   * Format: magic(4) + version(2) + length(2) + payload(68)
   * Total: 76 bytes
   *
   * @param account Account to extract resource usage from
   * @return AEXT tail bytes
   */
  static byte[] serializeAextTail(AccountCapsule account) {
    // AEXT v1 payload size: 8*8 (i64 fields) + 1 + 1 (booleans) + 2 (padding) = 68 bytes
    int payloadSize = 68;
    int totalSize = 4 + 2 + 2 + payloadSize; // magic + version + length + payload = 76 bytes
    byte[] result = new byte[totalSize];
    int offset = 0;

    // Magic: "AEXT" (0x41 0x45 0x58 0x54)
    result[offset++] = 0x41; // 'A'
    result[offset++] = 0x45; // 'E'
    result[offset++] = 0x58; // 'X'
    result[offset++] = 0x54; // 'T'

    // Version: 1 (u16 big-endian)
    result[offset++] = 0x00;
    result[offset++] = 0x01;

    // Length: 68 (u16 big-endian)
    result[offset++] = 0x00;
    result[offset++] = 0x44; // 0x44 = 68 in decimal

    // Payload: resource usage fields (all i64 big-endian except booleans)
    // netUsage (8 bytes)
    long netUsage = account.getNetUsage();
    offset = writeI64BigEndian(result, offset, netUsage);

    // freeNetUsage (8 bytes)
    long freeNetUsage = account.getFreeNetUsage();
    offset = writeI64BigEndian(result, offset, freeNetUsage);

    // energyUsage (8 bytes)
    long energyUsage = account.getEnergyUsage();
    offset = writeI64BigEndian(result, offset, energyUsage);

    // latestConsumeTime (8 bytes)
    long latestConsumeTime = account.getLatestConsumeTime();
    offset = writeI64BigEndian(result, offset, latestConsumeTime);

    // latestConsumeFreeTime (8 bytes)
    long latestConsumeFreeTime = account.getLatestConsumeFreeTime();
    offset = writeI64BigEndian(result, offset, latestConsumeFreeTime);

    // latestConsumeTimeForEnergy (8 bytes)
    long latestConsumeTimeForEnergy = account.getAccountResource().getLatestConsumeTimeForEnergy();
    offset = writeI64BigEndian(result, offset, latestConsumeTimeForEnergy);

    // netWindowSize (8 bytes) - use getWindowSize for logical units
    long netWindowSize = account.getWindowSize(BANDWIDTH);
    offset = writeI64BigEndian(result, offset, netWindowSize);

    // energyWindowSize (8 bytes)
    long energyWindowSize = account.getWindowSize(ENERGY);
    offset = writeI64BigEndian(result, offset, energyWindowSize);

    // netWindowOptimized (1 byte boolean)
    boolean netWindowOptimized = account.getWindowOptimized(BANDWIDTH);
    result[offset++] = (byte) (netWindowOptimized ? 0x01 : 0x00);

    // energyWindowOptimized (1 byte boolean)
    boolean energyWindowOptimized = account.getWindowOptimized(ENERGY);
    result[offset++] = (byte) (energyWindowOptimized ? 0x01 : 0x00);

    // Reserved/padding (2 bytes)
    result[offset++] = 0x00;
    result[offset++] = 0x00;

    logger.debug("Serialized AEXT v1: netUsage={}, freeNetUsage={}, energyUsage={}, times=[{},{},{}], windows=[{},{}], optimized=[{},{}]",
                 netUsage, freeNetUsage, energyUsage,
                 latestConsumeTime, latestConsumeFreeTime, latestConsumeTimeForEnergy,
                 netWindowSize, energyWindowSize,
                 netWindowOptimized, energyWindowOptimized);

    return result;
  }

  /**
   * Write an i64 value in big-endian format to the byte array.
   * Returns the new offset after writing.
   */
  private static int writeI64BigEndian(byte[] buffer, int offset, long value) {
    for (int i = 7; i >= 0; i--) {
      buffer[offset++] = (byte) (value >>> (i * 8));
    }
    return offset;
  }

  /**
   * Compute Keccak-256 hash of data.
   */
  static byte[] keccak256(byte[] data) {
    KeccakDigest digest = new KeccakDigest(256);
    if (data != null && data.length > 0) {
      digest.update(data, 0, data.length);
    }
    byte[] out = new byte[32];
    digest.doFinal(out, 0);
    return out;
  }

  /**
   * Convert long to 32-byte big-endian array.
   */
  static byte[] longToBytes32(long value) {
    byte[] bytes = new byte[32];
    // Store as big-endian in the last 8 bytes
    for (int i = 0; i < 8; i++) {
      bytes[31 - i] = (byte) (value >>> (i * 8));
    }
    return bytes;
  }

  /**
   * Convert long to 8-byte big-endian array.
   */
  static byte[] longToBytes8(long value) {
    byte[] bytes = new byte[8];
    for (int i = 0; i < 8; i++) {
      bytes[7 - i] = (byte) (value >>> (i * 8));
    }
    return bytes;
  }
}

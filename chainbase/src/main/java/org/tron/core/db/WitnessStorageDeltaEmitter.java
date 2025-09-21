package org.tron.core.db;

import java.nio.charset.StandardCharsets;
import org.bouncycastle.crypto.digests.KeccakDigest;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.core.capsule.WitnessCapsule;

/**
 * Utility class for emitting witness storage deltas to align embedded and remote CSV outputs.
 *
 * <p>This class implements the storage delta emission logic specified in the storage_deltas
 * planning document. It computes deterministic keys and value digests for witness metadata
 * changes to maintain parity between embedded (Java) and remote (Rust) execution paths.
 *
 * <p>Key derivation: keccak256(0x41 || owner_address || ":witness")
 * <p>Value derivation: keccak256(witness_record_bytes)
 */
public class WitnessStorageDeltaEmitter {

  private static final Logger logger = LoggerFactory.getLogger(WitnessStorageDeltaEmitter.class);

  private static final String WITNESS_TAG = ":witness";
  private static final byte TRON_ADDRESS_PREFIX = 0x41;

  /**
   * Check if witness storage delta emission is enabled.
   */
  public static boolean isEnabled() {
    String enabledProp = System.getProperty("embedded.exec.emitStorageChanges", "false");
    return "true".equalsIgnoreCase(enabledProp) && StateChangeRecorderContext.isEnabled();
  }

  /**
   * Emit storage delta for witness creation.
   *
   * @param ownerAddress The witness owner address (20 bytes)
   * @param newWitness The new witness capsule
   */
  public static void emitWitnessCreate(byte[] ownerAddress, WitnessCapsule newWitness) {
    if (!isEnabled()) {
      return;
    }

    try {
      byte[] witnessKey = computeMetadataKey(ownerAddress, WITNESS_TAG);
      byte[] oldValue = new byte[32]; // All zeros for creation
      byte[] newValue = digestWitnessRecord(newWitness);

      StateChangeRecorderContext.recordStorageChange(ownerAddress, witnessKey, oldValue, newValue);

      if (logger.isDebugEnabled()) {
        logger.debug("Emitted witness create storage delta - address: {}, key: {}, old: {}, new: {}",
                     bytesToHex(ownerAddress),
                     bytesToHex(witnessKey),
                     bytesToHex(oldValue),
                     bytesToHex(newValue));
      }
    } catch (Exception e) {
      logger.warn("Failed to emit witness create storage delta", e);
    }
  }

  /**
   * Emit storage delta for witness update.
   *
   * @param ownerAddress The witness owner address (20 bytes)
   * @param oldWitness The old witness capsule
   * @param newWitness The new witness capsule
   */
  public static void emitWitnessUpdate(byte[] ownerAddress, WitnessCapsule oldWitness,
                                      WitnessCapsule newWitness) {
    if (!isEnabled()) {
      return;
    }

    try {
      byte[] witnessKey = computeMetadataKey(ownerAddress, WITNESS_TAG);
      byte[] oldValue = digestWitnessRecord(oldWitness);
      byte[] newValue = digestWitnessRecord(newWitness);

      // Only emit if there's an actual change
      if (!java.util.Arrays.equals(oldValue, newValue)) {
        StateChangeRecorderContext.recordStorageChange(ownerAddress, witnessKey, oldValue, newValue);

        if (logger.isDebugEnabled()) {
          logger.debug("Emitted witness update storage delta - address: {}, key: {}, old: {}, new: {}",
                       bytesToHex(ownerAddress),
                       bytesToHex(witnessKey),
                       bytesToHex(oldValue),
                       bytesToHex(newValue));
        }
      }
    } catch (Exception e) {
      logger.warn("Failed to emit witness update storage delta", e);
    }
  }

  /**
   * Compute metadata key for storage delta emission.
   * Key derivation: keccak256(0x41 || address(20) || tag)
   */
  private static byte[] computeMetadataKey(byte[] address, String tag) {
    KeccakDigest keccak = new KeccakDigest(256);

    // Add Tron prefix (0x41)
    keccak.update(TRON_ADDRESS_PREFIX);

    // Add address (20 bytes)
    keccak.update(address, 0, address.length);

    // Add ASCII tag bytes
    byte[] tagBytes = tag.getBytes(StandardCharsets.UTF_8);
    keccak.update(tagBytes, 0, tagBytes.length);

    // Get hash and return as 32-byte key
    byte[] hash = new byte[32];
    keccak.doFinal(hash, 0);
    return hash;
  }

  /**
   * Compute digest of witness record bytes.
   * Uses the same serialization format as Rust: address(20) + url_len(4) + url + vote_count(8)
   */
  private static byte[] digestWitnessRecord(WitnessCapsule witness) {
    try {
      // Serialize witness record using canonical format
      byte[] serialized = serializeWitnessRecord(witness);

      // Hash the serialized data
      KeccakDigest keccak = new KeccakDigest(256);
      keccak.update(serialized, 0, serialized.length);

      byte[] hash = new byte[32];
      keccak.doFinal(hash, 0);
      return hash;
    } catch (Exception e) {
      logger.error("Failed to digest witness record", e);
      return new byte[32]; // Return zero hash on error
    }
  }

  /**
   * Serialize witness record using canonical format matching Rust implementation.
   * Format: address(20) + url_len(4) + url + vote_count(8)
   */
  private static byte[] serializeWitnessRecord(WitnessCapsule witness) {
    byte[] address = witness.getAddress().toByteArray();
    String url = witness.getUrl();
    long voteCount = witness.getVoteCount();

    // Convert URL to bytes
    byte[] urlBytes = url.getBytes(StandardCharsets.UTF_8);
    int urlLen = urlBytes.length;

    // Allocate buffer: 20 (address) + 4 (url_len) + url.length + 8 (vote_count)
    byte[] result = new byte[20 + 4 + urlLen + 8];
    int offset = 0;

    // Add address (20 bytes) - use last 20 bytes if address is 21 bytes with Tron prefix
    if (address.length == 21 && address[0] == TRON_ADDRESS_PREFIX) {
      System.arraycopy(address, 1, result, offset, 20);
    } else if (address.length == 20) {
      System.arraycopy(address, 0, result, offset, 20);
    } else {
      throw new IllegalArgumentException("Invalid address length: " + address.length);
    }
    offset += 20;

    // Add URL length (4 bytes, big-endian)
    result[offset++] = (byte) (urlLen >>> 24);
    result[offset++] = (byte) (urlLen >>> 16);
    result[offset++] = (byte) (urlLen >>> 8);
    result[offset++] = (byte) urlLen;

    // Add URL bytes
    System.arraycopy(urlBytes, 0, result, offset, urlLen);
    offset += urlLen;

    // Add vote count (8 bytes, big-endian)
    result[offset++] = (byte) (voteCount >>> 56);
    result[offset++] = (byte) (voteCount >>> 48);
    result[offset++] = (byte) (voteCount >>> 40);
    result[offset++] = (byte) (voteCount >>> 32);
    result[offset++] = (byte) (voteCount >>> 24);
    result[offset++] = (byte) (voteCount >>> 16);
    result[offset++] = (byte) (voteCount >>> 8);
    result[offset]   = (byte) voteCount;

    return result;
  }

  /**
   * Convert byte array to hex string for logging.
   */
  private static String bytesToHex(byte[] bytes) {
    StringBuilder sb = new StringBuilder();
    for (byte b : bytes) {
      sb.append(String.format("%02x", b));
    }
    return sb.toString();
  }
}
package org.tron.core.execution.reporting;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonParser;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.List;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;
import org.tron.common.BaseTest;
import org.tron.common.utils.ByteArray;
import org.tron.core.Constant;
import org.tron.core.Wallet;
import org.tron.core.config.args.Args;
import org.tron.core.db.TransactionTrace;
import org.tron.core.execution.spi.ExecutionSPI;

/**
 * Tests for ExecutionCsvRecordBuilder, specifically verifying that the provided
 * tokenId from Rust is used directly in CSV/domain output rather than falling
 * back to DynamicPropertiesStore.getTokenIdNum().
 */
public class ExecutionCsvRecordBuilderTest extends BaseTest {

  private static final String OWNER_ADDRESS;

  static {
    Args.setParam(new String[]{"--output-directory", dbPath()}, Constant.TEST_CONF);
    OWNER_ADDRESS = Wallet.getAddressPreFixString() + "abd4b9367799eaa3197fecb144eb71de1e049150";
  }

  private long originalTokenIdNum;

  @Before
  public void setUp() {
    originalTokenIdNum = dbManager.getDynamicPropertiesStore().getTokenIdNum();
    // Set TOKEN_ID_NUM to a different value so fallback usage is detectable
    dbManager.getDynamicPropertiesStore().saveTokenIdNum(9999999L);
  }

  @After
  public void cleanup() {
    // Restore original TOKEN_ID_NUM to avoid interfering with other tests
    dbManager.getDynamicPropertiesStore().saveTokenIdNum(originalTokenIdNum);
  }

  /**
   * When Trc10AssetIssued carries a non-empty tokenId, the CSV issuance domain
   * must use that value, not the DynamicPropertiesStore fallback (9999999).
   */
  @Test
  public void testExtractTrc10DomainsUsesProvidedTokenId() throws Exception {
    String providedTokenId = "1000042";
    byte[] ownerAddress = ByteArray.fromHexString(OWNER_ADDRESS);

    ExecutionSPI.Trc10AssetIssued assetIssued = new ExecutionSPI.Trc10AssetIssued(
        ownerAddress,
        "TestToken".getBytes(),
        "TT".getBytes(),
        1000000L,
        1,
        6,
        1,
        System.currentTimeMillis(),
        System.currentTimeMillis() + 86400000L,
        "Test description".getBytes(),
        "https://test.token".getBytes(),
        0L,
        0L,
        0L,
        0L,
        providedTokenId
    );

    ExecutionSPI.Trc10Change trc10Change = new ExecutionSPI.Trc10Change(assetIssued);
    List<ExecutionSPI.Trc10Change> trc10Changes = new ArrayList<>();
    trc10Changes.add(trc10Change);

    // Create a builder
    ExecutionCsvRecord.Builder builder = new ExecutionCsvRecord.Builder();

    // Invoke extractTrc10Domains via reflection (private static method)
    Method method = ExecutionCsvRecordBuilder.class.getDeclaredMethod(
        "extractTrc10Domains",
        ExecutionCsvRecord.Builder.class,
        List.class,
        TransactionTrace.class);
    method.setAccessible(true);
    method.invoke(null, builder, trc10Changes, null);

    // Build the record and check the issuance JSON
    ExecutionCsvRecord record = builder.build();
    String issuanceJson = record.getTrc10IssuanceChangesJson();

    assertNotNull("Issuance JSON should not be null", issuanceJson);
    // Parse JSON array and assert tokenId field values directly (not substring matching)
    JsonArray entries = JsonParser.parseString(issuanceJson).getAsJsonArray();
    assertTrue("Issuance JSON should have entries", entries.size() > 0);
    boolean foundProvidedTokenId = false;
    for (JsonElement elem : entries) {
      String tokenId = elem.getAsJsonObject().get("token_id").getAsString();
      assertFalse("tokenId field should NOT be the fallback value 9999999",
          "9999999".equals(tokenId));
      if (providedTokenId.equals(tokenId)) {
        foundProvidedTokenId = true;
      }
    }
    assertTrue("At least one entry should have tokenId=" + providedTokenId,
        foundProvidedTokenId);
    // 13 fields: owner, name, abbr, totalSupply, trxNum, precision, num,
    // startTime, endTime, description, url, freeAssetNetLimit, publicFreeAssetNetLimit
    assertEquals("Should have issuance changes", 13, record.getTrc10IssuanceChangeCount());
  }

  /**
   * When Trc10AssetIssued carries an empty tokenId and trace is null,
   * extractTrc10Domains should not crash. The tokenId remains empty because
   * the DynamicPropertiesStore fallback requires a non-null trace, and the
   * hex-of-name catch-block fallback is only reached on exception.
   */
  @Test
  public void testExtractTrc10DomainsEmptyTokenIdWithNullTrace() throws Exception {
    byte[] ownerAddress = ByteArray.fromHexString(OWNER_ADDRESS);

    ExecutionSPI.Trc10AssetIssued assetIssued = new ExecutionSPI.Trc10AssetIssued(
        ownerAddress,
        "TestToken".getBytes(),
        "TT".getBytes(),
        1000000L,
        1,
        6,
        1,
        System.currentTimeMillis(),
        System.currentTimeMillis() + 86400000L,
        "Test description".getBytes(),
        "https://test.token".getBytes(),
        0L,
        0L,
        0L,
        0L,
        "" // Empty: will attempt fallback
    );

    ExecutionSPI.Trc10Change trc10Change = new ExecutionSPI.Trc10Change(assetIssued);
    List<ExecutionSPI.Trc10Change> trc10Changes = new ArrayList<>();
    trc10Changes.add(trc10Change);

    ExecutionCsvRecord.Builder builder = new ExecutionCsvRecord.Builder();

    Method method = ExecutionCsvRecordBuilder.class.getDeclaredMethod(
        "extractTrc10Domains",
        ExecutionCsvRecord.Builder.class,
        List.class,
        TransactionTrace.class);
    method.setAccessible(true);
    // Should not throw even with null trace
    method.invoke(null, builder, trc10Changes, null);

    ExecutionCsvRecord record = builder.build();
    String issuanceJson = record.getTrc10IssuanceChangesJson();
    assertNotNull("Issuance JSON should not be null even with empty tokenId and null trace",
        issuanceJson);
    // Parse JSON and verify token_id field is present and not the fallback value
    JsonArray entries = JsonParser.parseString(issuanceJson).getAsJsonArray();
    assertTrue("Issuance JSON should have entries", entries.size() > 0);
    for (JsonElement elem : entries) {
      assertTrue("Every issuance entry must have a token_id field",
          elem.getAsJsonObject().has("token_id"));
      String tokenId = elem.getAsJsonObject().get("token_id").getAsString();
      assertFalse("token_id field should NOT be the DynamicPropertiesStore fallback 9999999",
          "9999999".equals(tokenId));
      // With null trace, tokenId stays empty (DynamicPropertiesStore unreachable)
      assertTrue("token_id should be empty when trace is null",
          tokenId.isEmpty());
    }
    // 13 fields: see testExtractTrc10DomainsUsesProvidedTokenId for field list
    assertEquals("Should still have issuance changes", 13, record.getTrc10IssuanceChangeCount());
  }
}

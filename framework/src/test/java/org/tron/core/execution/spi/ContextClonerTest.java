package org.tron.core.execution.spi;

import static org.junit.Assert.*;

import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.mockito.Mock;
import org.mockito.junit.MockitoJUnitRunner;
import org.tron.common.runtime.ProgramResult;
import org.tron.core.capsule.BlockCapsule;
import org.tron.core.capsule.TransactionCapsule;
import org.tron.core.db.TransactionContext;
import org.tron.core.store.StoreFactory;

/**
 * Test suite for ContextCloner to ensure proper transaction context isolation
 * in shadow execution mode. These tests are critical for validating that
 * parallel execution paths don't interfere with each other.
 */
@RunWith(MockitoJUnitRunner.class)
public class ContextClonerTest {

  private ContextCloner contextCloner;
  
  @Mock private BlockCapsule mockBlockCap;
  @Mock private TransactionCapsule mockTrxCap;
  @Mock private StoreFactory mockStoreFactory;

  private TransactionContext originalContext;

  @Before
  public void setUp() {
    contextCloner = new ContextCloner();
    
    // Create a sample original context
    originalContext = new TransactionContext(
        mockBlockCap,
        mockTrxCap,
        mockStoreFactory,
        false, // isStatic
        true   // eventPluginLoaded
    );
    
    // Set up a ProgramResult with some initial state
    ProgramResult originalResult = new ProgramResult();
    originalContext.setProgramResult(originalResult);
  }

  @Test
  public void testDeepCloneCreatesIndependentContext() {
    // Perform the cloning
    TransactionContext cloned = contextCloner.deepClone(originalContext);

    // Verify cloned context is not the same instance
    assertNotSame("Cloned context should be a different instance", originalContext, cloned);
    
    // Verify primitive fields are copied correctly
    assertEquals("isStatic should be copied", originalContext.isStatic(), cloned.isStatic());
    assertEquals("eventPluginLoaded should be copied", 
        originalContext.isEventPluginLoaded(), cloned.isEventPluginLoaded());
  }

  @Test
  public void testImmutableFieldsAreShared() {
    TransactionContext cloned = contextCloner.deepClone(originalContext);

    // Verify immutable fields are shared (same references)
    assertSame("BlockCapsule should be shared", originalContext.getBlockCap(), cloned.getBlockCap());
    assertSame("TransactionCapsule should be shared", originalContext.getTrxCap(), cloned.getTrxCap());
    assertSame("StoreFactory should be shared", originalContext.getStoreFactory(), cloned.getStoreFactory());
  }

  @Test
  public void testProgramResultIsCloned() {
    TransactionContext cloned = contextCloner.deepClone(originalContext);

    // Verify ProgramResult instances are different
    assertNotSame("ProgramResult should be cloned", 
        originalContext.getProgramResult(), cloned.getProgramResult());
    
    // Both should be non-null
    assertNotNull("Original ProgramResult should not be null", originalContext.getProgramResult());
    assertNotNull("Cloned ProgramResult should not be null", cloned.getProgramResult());
  }

  @Test
  public void testProgramResultIndependence() {
    TransactionContext cloned = contextCloner.deepClone(originalContext);
    
    ProgramResult originalResult = originalContext.getProgramResult();
    ProgramResult clonedResult = cloned.getProgramResult();

    // Modify one ProgramResult - this simulates execution modifying state
    originalResult.spendEnergy(1000);
    originalResult.spendEnergy(500);
    
    // Verify the other ProgramResult is unaffected
    assertEquals("Cloned result should start with zero energy", 0, clonedResult.getEnergyUsed());
    
    // Modify cloned result
    clonedResult.spendEnergy(2000);
    
    // Verify original is unaffected by cloned modifications
    assertEquals("Original should maintain its energy value", 1500, originalResult.getEnergyUsed());
    assertEquals("Cloned should have its own energy value", 2000, clonedResult.getEnergyUsed());
  }

  @Test
  public void testValidateIsolation() {
    TransactionContext cloned1 = contextCloner.deepClone(originalContext);
    TransactionContext cloned2 = contextCloner.deepClone(originalContext);

    // Validate isolation between cloned contexts
    boolean isolated = contextCloner.validateIsolation(cloned1, cloned2);
    assertTrue("Cloned contexts should be properly isolated", isolated);
  }

  @Test
  public void testValidateIsolationDetectsSharedMutableState() {
    TransactionContext cloned = contextCloner.deepClone(originalContext);
    
    // Manually break isolation by sharing ProgramResult (simulates bug)
    cloned.setProgramResult(originalContext.getProgramResult());
    
    // Validation should detect the problem
    boolean isolated = contextCloner.validateIsolation(originalContext, cloned);
    assertFalse("Validation should detect shared mutable state", isolated);
  }

  @Test
  public void testCreateMultipleClones() {
    int cloneCount = 3;
    TransactionContext[] clones = contextCloner.createMultipleClones(originalContext, cloneCount);

    assertEquals("Should create requested number of clones", cloneCount, clones.length);
    
    // Verify each clone is independent
    for (int i = 0; i < cloneCount; i++) {
      assertNotNull("Clone " + i + " should not be null", clones[i]);
      assertNotSame("Clone " + i + " should be different from original", originalContext, clones[i]);
      
      // Verify isolation from other clones
      for (int j = i + 1; j < cloneCount; j++) {
        boolean isolated = contextCloner.validateIsolation(clones[i], clones[j]);
        assertTrue("Clone " + i + " and " + j + " should be isolated", isolated);
      }
    }
  }

  @Test(expected = IllegalArgumentException.class)
  public void testDeepCloneWithNullContextThrowsException() {
    contextCloner.deepClone(null);
  }

  @Test(expected = IllegalArgumentException.class)
  public void testCreateMultipleClonesWithInvalidCountThrowsException() {
    contextCloner.createMultipleClones(originalContext, 0);
  }

  @Test
  public void testCloneWithNullProgramResult() {
    // Set up context with null ProgramResult
    originalContext.setProgramResult(null);
    
    TransactionContext cloned = contextCloner.deepClone(originalContext);
    
    // Should create a new ProgramResult
    assertNotNull("Cloned context should have a ProgramResult", cloned.getProgramResult());
    assertNull("Original should still have null ProgramResult", originalContext.getProgramResult());
  }

  @Test
  public void testConcurrentCloning() throws InterruptedException {
    final int threadCount = 10;
    final TransactionContext[] results = new TransactionContext[threadCount];
    final Thread[] threads = new Thread[threadCount];
    
    // Create threads that clone concurrently
    for (int i = 0; i < threadCount; i++) {
      final int index = i;
      threads[i] = new Thread(() -> {
        results[index] = contextCloner.deepClone(originalContext);
      });
    }
    
    // Start all threads
    for (Thread thread : threads) {
      thread.start();
    }
    
    // Wait for all to complete
    for (Thread thread : threads) {
      thread.join();
    }
    
    // Verify all results are valid and isolated
    for (int i = 0; i < threadCount; i++) {
      assertNotNull("Result " + i + " should not be null", results[i]);
      assertNotSame("Result " + i + " should be different from original", originalContext, results[i]);
      
      // Verify isolation from other results
      for (int j = i + 1; j < threadCount; j++) {
        boolean isolated = contextCloner.validateIsolation(results[i], results[j]);
        assertTrue("Results " + i + " and " + j + " should be isolated", isolated);
      }
    }
  }
}
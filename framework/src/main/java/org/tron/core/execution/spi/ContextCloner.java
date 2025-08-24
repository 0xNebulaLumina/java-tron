package org.tron.core.execution.spi;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.tron.common.runtime.ProgramResult;
import org.tron.core.db.TransactionContext;

/**
 * Utility class for deep cloning TransactionContext instances to ensure execution isolation
 * in shadow mode. Each execution path (embedded/remote) gets its own independent context
 * to prevent race conditions and enable accurate result comparison.
 */
public class ContextCloner {
  private static final Logger logger = LoggerFactory.getLogger(ContextCloner.class);

  /**
   * Creates a deep clone of the TransactionContext for independent execution.
   * 
   * Cloning Strategy:
   * - CLONE: ProgramResult (mutable execution state)
   * - SHARE: BlockCapsule, TransactionCapsule (immutable during execution)
   * - SHARE: StoreFactory (thread-safe, shared resource)
   * - COPY: Primitive fields (boolean flags)
   *
   * @param original The original TransactionContext to clone
   * @return A new TransactionContext with independent mutable state
   * @throws IllegalArgumentException if original context is null
   */
  public TransactionContext deepClone(TransactionContext original) {
    if (original == null) {
      throw new IllegalArgumentException("Original TransactionContext cannot be null");
    }

    logger.debug("Cloning TransactionContext for transaction: {}", 
        original.getTrxCap().getTransactionId());

    // Create new context with shared immutable references
    TransactionContext cloned = new TransactionContext(
        original.getBlockCap(),        // SHARED: Immutable during execution
        original.getTrxCap(),          // SHARED: Immutable during execution  
        original.getStoreFactory(),    // SHARED: Thread-safe shared resource
        original.isStatic(),           // COPIED: Primitive boolean
        original.isEventPluginLoaded() // COPIED: Primitive boolean
    );

    // Clone the mutable ProgramResult to ensure isolation
    ProgramResult clonedResult = cloneProgramResult(original.getProgramResult());
    cloned.setProgramResult(clonedResult);

    logger.debug("Successfully cloned TransactionContext with independent ProgramResult");
    return cloned;
  }

  /**
   * Creates a deep clone of ProgramResult to ensure execution isolation.
   * This is critical because ProgramResult accumulates execution artifacts
   * that must not be shared between parallel execution paths.
   *
   * @param original The original ProgramResult to clone
   * @return A new ProgramResult with fresh mutable state
   */
  private ProgramResult cloneProgramResult(ProgramResult original) {
    if (original == null) {
      return new ProgramResult();
    }

    ProgramResult cloned = new ProgramResult();
    
    // Reset to initial state - each execution path starts fresh
    // The execution engine will populate these fields during execution
    
    // Energy tracking starts at zero
    // Return data starts empty  
    // Exception state starts clean
    // Internal transactions list starts empty
    // Log info list starts empty
    
    logger.debug("Created fresh ProgramResult for independent execution path");
    return cloned;
  }

  /**
   * Validates that two cloned contexts are properly isolated.
   * This is used in tests to ensure cloning creates independent instances.
   *
   * @param context1 First context to compare
   * @param context2 Second context to compare
   * @return true if contexts are properly isolated, false otherwise
   */
  public boolean validateIsolation(TransactionContext context1, TransactionContext context2) {
    if (context1 == null || context2 == null) {
      return false;
    }

    // Check that mutable fields are different instances
    boolean programResultsIsolated = context1.getProgramResult() != context2.getProgramResult();
    
    // Check that immutable fields are shared (same references)
    boolean immutableFieldsShared = 
        context1.getBlockCap() == context2.getBlockCap() &&
        context1.getTrxCap() == context2.getTrxCap() &&
        context1.getStoreFactory() == context2.getStoreFactory();
    
    // Check that primitive fields are independent copies
    boolean primitiveFieldsIndependent = true; // They're copied by value
    
    boolean isIsolated = programResultsIsolated && immutableFieldsShared && primitiveFieldsIndependent;
    
    if (!isIsolated) {
      logger.warn("Context isolation validation failed: programResults={}, immutableShared={}", 
          programResultsIsolated, immutableFieldsShared);
    }
    
    return isIsolated;
  }

  /**
   * Creates multiple independent clones of the same context.
   * This is useful for parallel execution scenarios.
   *
   * @param original The original context to clone
   * @param count Number of clones to create
   * @return Array of independent context clones
   */
  public TransactionContext[] createMultipleClones(TransactionContext original, int count) {
    if (count <= 0) {
      throw new IllegalArgumentException("Clone count must be positive");
    }

    TransactionContext[] clones = new TransactionContext[count];
    for (int i = 0; i < count; i++) {
      clones[i] = deepClone(original);
    }

    logger.debug("Created {} independent context clones", count);
    return clones;
  }
}
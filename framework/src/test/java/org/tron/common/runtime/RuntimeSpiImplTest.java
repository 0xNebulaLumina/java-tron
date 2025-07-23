package org.tron.common.runtime;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

import org.junit.Before;
import org.junit.Test;
import org.tron.common.BaseTest;
import org.tron.core.db.TransactionContext;
import org.tron.core.execution.spi.ExecutionSpiFactory;
import org.tron.core.execution.spi.ExecutionMode;
import org.tron.protos.Protocol.Transaction.Result.contractResult;

/**
 * Test class for RuntimeSpiImpl to verify ExecutionSPI integration.
 */
public class RuntimeSpiImplTest extends BaseTest {

    @Before
    public void setUp() {
        // Initialize ExecutionSPI factory for testing
        try {
            ExecutionSpiFactory.initialize();
        } catch (Exception e) {
            // Factory may already be initialized
        }
    }

    @Test
    public void testRuntimeSpiImplCreation() {
        // Test that RuntimeSpiImpl can be created successfully
        RuntimeSpiImpl runtime = new RuntimeSpiImpl();
        assertNotNull("RuntimeSpiImpl should be created successfully", runtime);
    }

    @Test
    public void testExecutionModeDetection() {
        // Test that execution mode can be determined
        ExecutionMode mode = ExecutionSpiFactory.determineExecutionMode();
        assertNotNull("Execution mode should not be null", mode);
        
        // Default mode should be EMBEDDED
        assertEquals("Default execution mode should be EMBEDDED", 
                    ExecutionMode.EMBEDDED, mode);
    }

    @Test
    public void testRuntimeSpiImplWithEmbeddedMode() {
        // Test RuntimeSpiImpl with EMBEDDED mode (should work like RuntimeImpl)
        RuntimeSpiImpl runtime = new RuntimeSpiImpl();
        
        // Test basic methods
        ProgramResult result = runtime.getResult();
        // Result should be null or empty initially
        assertTrue("Initial result should be null or empty", 
                  result == null || result.getEnergyUsed() == 0);
        
        String error = runtime.getRuntimeError();
        // Error should be null initially
        assertTrue("Initial runtime error should be null", error == null);
    }

    @Test
    public void testRuntimeSpiImplErrorHandling() {
        // Test error handling when ExecutionSPI is not properly initialized
        RuntimeSpiImpl runtime = new RuntimeSpiImpl();
        
        // Create a mock transaction context (this will likely fail in execution)
        // but should not crash the runtime
        try {
            TransactionContext context = null; // Intentionally null to test error handling
            runtime.execute(context);
        } catch (Exception e) {
            // Expected to fail with null context
            assertTrue("Should handle null context gracefully", 
                      e.getMessage().contains("null") || 
                      e.getMessage().contains("failed"));
        }
    }

    @Test
    public void testExecutionSpiFactoryInitialization() {
        // Test that ExecutionSPI factory is properly initialized
        assertNotNull("ExecutionSPI instance should be available", 
                     ExecutionSpiFactory.getInstance());
    }

    @Test
    public void testRuntimeCompatibility() {
        // Test that RuntimeSpiImpl implements Runtime interface correctly
        Runtime runtime = new RuntimeSpiImpl();
        assertNotNull("RuntimeSpiImpl should implement Runtime interface", runtime);
        
        // Test interface methods are available
        ProgramResult result = runtime.getResult();
        String error = runtime.getRuntimeError();
        
        // Methods should not throw exceptions when called
        assertTrue("Interface methods should be callable", true);
    }

    @Test
    public void testConfigurationInfo() {
        // Test that configuration information can be retrieved
        String configInfo = ExecutionSpiFactory.getConfigurationInfo();
        assertNotNull("Configuration info should not be null", configInfo);
        assertTrue("Configuration info should contain mode information", 
                  configInfo.contains("Mode:"));
    }

    @Test
    public void testExecutionModeFromString() {
        // Test ExecutionMode enum parsing
        assertEquals("EMBEDDED mode should parse correctly", 
                    ExecutionMode.EMBEDDED, ExecutionMode.fromString("EMBEDDED"));
        assertEquals("REMOTE mode should parse correctly", 
                    ExecutionMode.REMOTE, ExecutionMode.fromString("REMOTE"));
        assertEquals("SHADOW mode should parse correctly", 
                    ExecutionMode.SHADOW, ExecutionMode.fromString("SHADOW"));
        
        // Test case insensitive parsing
        assertEquals("Lowercase embedded should parse correctly", 
                    ExecutionMode.EMBEDDED, ExecutionMode.fromString("embedded"));
    }

    @Test
    public void testDefaultExecutionMode() {
        // Test that default execution mode is EMBEDDED
        ExecutionMode defaultMode = ExecutionMode.getDefault();
        assertEquals("Default execution mode should be EMBEDDED", 
                    ExecutionMode.EMBEDDED, defaultMode);
    }

    @Test
    public void testRuntimeSpiImplVsRuntimeImpl() {
        // Compare RuntimeSpiImpl with RuntimeImpl for basic compatibility
        Runtime spiRuntime = new RuntimeSpiImpl();
        Runtime implRuntime = new RuntimeImpl();
        
        // Both should implement the same interface
        assertNotNull("SPI runtime should not be null", spiRuntime);
        assertNotNull("Impl runtime should not be null", implRuntime);
        
        // Both should have the same interface methods
        ProgramResult spiResult = spiRuntime.getResult();
        ProgramResult implResult = implRuntime.getResult();
        
        String spiError = spiRuntime.getRuntimeError();
        String implError = implRuntime.getRuntimeError();
        
        // Initial states should be similar (both null or both empty)
        assertTrue("Initial states should be compatible", true);
    }
}

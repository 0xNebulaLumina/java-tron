package org.tron.common.runtime;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertTrue;

import org.junit.Before;
import org.junit.Test;
import org.tron.core.execution.spi.ExecutionSpiFactory;
import org.tron.core.execution.spi.ExecutionMode;

/**
 * Test class for RuntimeSpiImpl to verify ExecutionSPI integration.
 * This is a simple JUnit test without Spring context to avoid initialization issues.
 */
public class RuntimeSpiImplTest {

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
    public void testExecutionModeDetection() {
        // Test that execution mode can be determined
        ExecutionMode mode = ExecutionSpiFactory.determineExecutionMode();
        assertNotNull("Execution mode should not be null", mode);

        // Default mode should be EMBEDDED
        assertEquals("Default execution mode should be EMBEDDED",
                    ExecutionMode.EMBEDDED, mode);
    }

    @Test
    public void testExecutionSpiFactoryInitialization() {
        // Test that ExecutionSPI factory is properly initialized
        assertNotNull("ExecutionSPI instance should be available",
                     ExecutionSpiFactory.getInstance());
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
}

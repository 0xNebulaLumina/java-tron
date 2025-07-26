use std::collections::HashMap;
use async_trait::async_trait;
use anyhow::Result;
use tracing::{info, warn, error};

/// Trait that all backend modules must implement
#[async_trait]
pub trait Module: Send + Sync {
    /// Module name (used for logging and configuration)
    fn name(&self) -> &str;
    
    /// Module version
    fn version(&self) -> &str;
    
    /// Initialize the module (called once at startup)
    async fn init(&mut self) -> Result<()>;
    
    /// Start the module (called after all modules are initialized)
    async fn start(&mut self) -> Result<()>;
    
    /// Stop the module (called during shutdown)
    async fn stop(&mut self) -> Result<()>;
    
    /// Health check for the module
    async fn health(&self) -> ModuleHealth;
    
    /// Get module-specific metrics
    fn metrics(&self) -> HashMap<String, f64>;
}

#[derive(Debug, Clone)]
pub struct ModuleHealth {
    pub status: HealthStatus,
    pub message: String,
    pub details: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

impl ModuleHealth {
    pub fn healthy() -> Self {
        Self {
            status: HealthStatus::Healthy,
            message: "Module is healthy".to_string(),
            details: HashMap::new(),
        }
    }
    
    pub fn degraded(message: &str) -> Self {
        Self {
            status: HealthStatus::Degraded,
            message: message.to_string(),
            details: HashMap::new(),
        }
    }
    
    pub fn unhealthy(message: &str) -> Self {
        Self {
            status: HealthStatus::Unhealthy,
            message: message.to_string(),
            details: HashMap::new(),
        }
    }
    
    pub fn with_details(mut self, details: HashMap<String, String>) -> Self {
        self.details = details;
        self
    }
}

/// Module manager handles the lifecycle of all modules
pub struct ModuleManager {
    modules: HashMap<String, Box<dyn Module>>,
    startup_order: Vec<String>,
}

impl ModuleManager {
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            startup_order: Vec::new(),
        }
    }
    
    /// Register a module
    pub fn register(&mut self, name: &str, module: Box<dyn Module>) {
        info!("Registering module: {} v{}", module.name(), module.version());
        self.startup_order.push(name.to_string());
        self.modules.insert(name.to_string(), module);
    }
    
    /// Initialize all modules in registration order
    pub async fn init_all(&mut self) -> Result<()> {
        for name in &self.startup_order {
            if let Some(module) = self.modules.get_mut(name) {
                info!("Initializing module: {}", module.name());
                if let Err(e) = module.init().await {
                    error!("Failed to initialize module {}: {}", module.name(), e);
                    return Err(e);
                }
            }
        }
        Ok(())
    }
    
    /// Start all modules in registration order
    pub async fn start_all(&mut self) -> Result<()> {
        // First initialize all modules
        self.init_all().await?;
        
        // Then start them
        for name in &self.startup_order {
            if let Some(module) = self.modules.get_mut(name) {
                info!("Starting module: {}", module.name());
                if let Err(e) = module.start().await {
                    error!("Failed to start module {}: {}", module.name(), e);
                    return Err(e);
                }
            }
        }
        Ok(())
    }
    
    /// Stop all modules in reverse order
    pub async fn stop_all(&mut self) -> Result<()> {
        let mut stop_order = self.startup_order.clone();
        stop_order.reverse();
        
        for name in &stop_order {
            if let Some(module) = self.modules.get_mut(name) {
                info!("Stopping module: {}", module.name());
                if let Err(e) = module.stop().await {
                    warn!("Failed to stop module {}: {}", module.name(), e);
                    // Continue stopping other modules even if one fails
                }
            }
        }
        Ok(())
    }
    
    /// Get a module by name
    pub fn get(&self, name: &str) -> Option<&Box<dyn Module>> {
        self.modules.get(name)
    }
    
    /// Get a mutable reference to a module by name
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Box<dyn Module>> {
        self.modules.get_mut(name)
    }
    
    /// Get health status of all modules
    pub async fn health_all(&self) -> HashMap<String, ModuleHealth> {
        let mut health_map = HashMap::new();
        
        for (name, module) in &self.modules {
            let health = module.health().await;
            health_map.insert(name.clone(), health);
        }
        
        health_map
    }
    
    /// Get metrics from all modules
    pub fn metrics_all(&self) -> HashMap<String, HashMap<String, f64>> {
        let mut metrics_map = HashMap::new();
        
        for (name, module) in &self.modules {
            let metrics = module.metrics();
            if !metrics.is_empty() {
                metrics_map.insert(name.clone(), metrics);
            }
        }
        
        metrics_map
    }
    
    /// Get list of registered module names
    pub fn module_names(&self) -> Vec<String> {
        self.startup_order.clone()
    }
    
    /// Get list of registered module versions
    pub fn module_versions(&self) -> HashMap<String, String> {
        let mut versions = HashMap::new();
        for (name, module) in &self.modules {
            versions.insert(name.clone(), module.version().to_string());
        }
        versions
    }

    /// Get a reference to the raw modules map for advanced access patterns
    pub fn modules(&self) -> &HashMap<String, Box<dyn Module>> {
        &self.modules
    }
} 
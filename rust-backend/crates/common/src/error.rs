use thiserror::Error;

pub type BackendResult<T> = Result<T, BackendError>;

#[derive(Error, Debug)]
pub enum BackendError {
    #[error("Storage error: {0}")]
    Storage(String),
    
    #[error("Execution error: {0}")]
    Execution(String),
    
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("Module error: {module} - {message}")]
    Module { module: String, message: String },
    
    #[error("gRPC error: {0}")]
    Grpc(#[from] tonic::Status),
    
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("Internal error: {0}")]
    Internal(String),
}

impl BackendError {
    pub fn storage(msg: &str) -> Self {
        Self::Storage(msg.to_string())
    }
    
    pub fn execution(msg: &str) -> Self {
        Self::Execution(msg.to_string())
    }
    
    pub fn config(msg: &str) -> Self {
        Self::Config(msg.to_string())
    }
    
    pub fn module(module: &str, msg: &str) -> Self {
        Self::Module {
            module: module.to_string(),
            message: msg.to_string(),
        }
    }
    
    pub fn internal(msg: &str) -> Self {
        Self::Internal(msg.to_string())
    }
}

impl From<BackendError> for tonic::Status {
    fn from(err: BackendError) -> Self {
        match err {
            BackendError::Storage(msg) => tonic::Status::internal(format!("Storage error: {}", msg)),
            BackendError::Execution(msg) => tonic::Status::internal(format!("Execution error: {}", msg)),
            BackendError::Config(msg) => tonic::Status::invalid_argument(format!("Config error: {}", msg)),
            BackendError::Module { module, message } => tonic::Status::internal(format!("Module {} error: {}", module, message)),
            BackendError::Grpc(status) => status,
            BackendError::Io(e) => tonic::Status::internal(format!("I/O error: {}", e)),
            BackendError::Serialization(e) => tonic::Status::internal(format!("Serialization error: {}", e)),
            BackendError::Internal(msg) => tonic::Status::internal(msg),
        }
    }
} 
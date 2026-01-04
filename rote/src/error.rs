use std::io;

/// Error types for the Rote application.
#[derive(Debug)]
pub enum RoteError {
    /// I/O error (file, terminal, process)
    Io(io::Error),
    /// Configuration error (invalid YAML, missing fields)
    Config(String),
    /// Service dependency error (circular deps, missing deps)
    Dependency(String),
    /// Process spawn error
    Spawn { service: String, source: io::Error },
}

impl std::fmt::Display for RoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RoteError::Io(e) => write!(f, "I/O error: {e}"),
            RoteError::Config(msg) => write!(f, "Configuration error: {msg}"),
            RoteError::Dependency(msg) => write!(f, "Dependency error: {msg}"),
            RoteError::Spawn { service, source } => {
                write!(f, "Failed to spawn service '{service}': {source}")
            }
        }
    }
}

impl std::error::Error for RoteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RoteError::Io(e) => Some(e),
            RoteError::Spawn { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<io::Error> for RoteError {
    fn from(err: io::Error) -> Self {
        RoteError::Io(err)
    }
}

impl From<serde_yaml::Error> for RoteError {
    fn from(err: serde_yaml::Error) -> Self {
        RoteError::Config(err.to_string())
    }
}

impl From<RoteError> for io::Error {
    fn from(err: RoteError) -> Self {
        match err {
            RoteError::Io(e) => e,
            RoteError::Config(msg) => io::Error::new(io::ErrorKind::InvalidData, msg),
            RoteError::Dependency(msg) => io::Error::new(io::ErrorKind::InvalidInput, msg),
            RoteError::Spawn { service, source } => io::Error::new(
                source.kind(),
                format!("Failed to spawn service '{service}': {source}"),
            ),
        }
    }
}

/// A specialized Result type for Rote operations.
pub type Result<T> = std::result::Result<T, RoteError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn test_io_error_display() {
        let err = RoteError::Io(io::Error::new(io::ErrorKind::NotFound, "file not found"));
        let display = format!("{err}");
        assert!(display.contains("I/O error"));
        assert!(display.contains("file not found"));
    }

    #[test]
    fn test_config_error_display() {
        let err = RoteError::Config("invalid service name".to_string());
        let display = format!("{err}");
        assert!(display.contains("Configuration error"));
        assert!(display.contains("invalid service name"));
    }

    #[test]
    fn test_dependency_error_display() {
        let err = RoteError::Dependency("circular dependency detected".to_string());
        let display = format!("{err}");
        assert!(display.contains("Dependency error"));
        assert!(display.contains("circular dependency"));
    }

    #[test]
    fn test_spawn_error_display() {
        let err = RoteError::Spawn {
            service: "web-server".to_string(),
            source: io::Error::new(io::ErrorKind::NotFound, "command not found"),
        };
        let display = format!("{err}");
        assert!(display.contains("Failed to spawn"));
        assert!(display.contains("web-server"));
        assert!(display.contains("command not found"));
    }

    #[test]
    fn test_from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let rote_err: RoteError = io_err.into();
        assert!(matches!(rote_err, RoteError::Io(_)));
    }

    #[test]
    fn test_error_source() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "not found");
        let rote_err = RoteError::Io(io_err);
        assert!(rote_err.source().is_some());

        let config_err = RoteError::Config("bad config".to_string());
        assert!(config_err.source().is_none());
    }
}

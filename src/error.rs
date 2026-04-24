use pyo3::exceptions::{PyOSError, PyValueError, PyPermissionError};
use pyo3::prelude::*;

/// Unified error type for membridge operations.
///
/// Variants map directly to Python exceptions via [`From<ShmError> for PyErr`]:
/// - [`ShmError::Os`]         → [`PyOSError`]
/// - [`ShmError::InvalidArg`] → [`PyValueError`]
/// - [`ShmError::Permission`] → [`PyPermissionError`]
#[derive(Debug)]
pub enum ShmError {
    /// An OS-level error (errno on Unix, Win32 error on Windows).
    Os(String),

    /// A caller-supplied argument is invalid (e.g. `size == 0`).
    InvalidArg(String),

    /// The calling process lacks the required privileges.
    Permission(String),
}

impl std::fmt::Display for ShmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShmError::Os(msg)          => write!(f, "OS error: {msg}"),
            ShmError::InvalidArg(msg)  => write!(f, "Invalid argument: {msg}"),
            ShmError::Permission(msg)  => write!(f, "Permission denied: {msg}"),
        }
    }
}

impl std::error::Error for ShmError {}

impl From<ShmError> for PyErr {
    fn from(e: ShmError) -> PyErr {
        match e {
            ShmError::Os(msg)          => PyOSError::new_err(msg),
            ShmError::InvalidArg(msg)  => PyValueError::new_err(msg),
            ShmError::Permission(msg)  => PyPermissionError::new_err(msg),
        }
    }
}

// Unix: convert nix::errno::Errno → ShmError::Os
#[cfg(unix)]
impl From<nix::errno::Errno> for ShmError {
    fn from(e: nix::errno::Errno) -> Self {
        ShmError::Os(e.to_string())
    }
}

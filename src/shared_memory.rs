use pyo3::prelude::*;

use crate::error::ShmError;
use crate::mapped_view::MappedView;
use crate::platform::PlatformHandle;

/// A handle to a named cross-platform shared memory segment.
///
/// Delegates all OS primitives to [`PlatformHandle`]:
/// - Unix  — `shm_open` + `ftruncate` + `mmap`
/// - Windows — `CreateFileMappingA` + `MapViewOfFile`
///
/// # Name format
///
/// Names must start with `'/'` and contain at least one additional character,
/// e.g. `"/my_segment"`. On Windows the leading `'/'` is stripped internally
/// before passing to Win32 APIs.
///
/// # Cleanup
///
/// On Unix, call [`remove`](SharedMemory::remove) once when the segment is no
/// longer needed (`shm_unlink`). On Windows this is a no-op — the kernel
/// reclaims the object automatically when all handles are closed.
#[pyclass]
pub struct SharedMemory {
    handle:     PlatformHandle,
    /// `true` for the process that called [`create`](SharedMemory::create).
    is_creator: bool,
}

// SAFETY: `PlatformHandle` is `Send + Sync` on both platforms.
unsafe impl Send for SharedMemory {}
unsafe impl Sync for SharedMemory {}

#[pymethods]
impl SharedMemory {
    /// Creates a new named shared memory segment of `size` bytes.
    ///
    /// # Errors
    ///
    /// - [`PyValueError`] if `size == 0` or `name` is invalid.
    /// - [`PyOSError`] on OS-level failure.
    #[staticmethod]
    pub fn create(name: &str, size: usize) -> PyResult<Self> {
        if size == 0 {
            return Err(ShmError::InvalidArg("size must be > 0".into()).into());
        }
        validate_name(name)?;

        let handle = PlatformHandle::create(name, size)
            .map_err(PyErr::from)?;

        Ok(SharedMemory { handle, is_creator: true })
    }

    /// Opens an existing named shared memory segment.
    ///
    /// On Unix the size is read from `fstat`. On Windows `size` must be
    /// supplied explicitly and must match the value used at creation time.
    ///
    /// # Errors
    ///
    /// - [`PyValueError`] if `name` is invalid.
    /// - [`PyOSError`] if the segment does not exist or the call fails.
    #[staticmethod]
    #[pyo3(signature = (name, size=None))]
    pub fn open(name: &str, size: Option<usize>) -> PyResult<Self> {
        validate_name(name)?;

        #[cfg(unix)]
        let handle = PlatformHandle::open(name)
            .map_err(PyErr::from)?;

        #[cfg(windows)]
        let handle = {
            let sz = size.ok_or_else(|| {
                ShmError::InvalidArg(
                    "size is required on Windows (Win32 does not expose segment size)".into()
                )
            })?;
            PlatformHandle::open(name, sz).map_err(PyErr::from)?
        };

        // Suppress unused variable warning on Unix.
        let _ = size;

        Ok(SharedMemory { handle, is_creator: false })
    }

    /// Removes the named segment.
    ///
    /// On Unix this calls `shm_unlink`; existing mappings remain valid until
    /// dropped. On Windows this is a no-op.
    ///
    /// # Errors
    ///
    /// - [`PyValueError`] if `name` is invalid.
    /// - [`PyOSError`] on Unix syscall failure.
    #[staticmethod]
    pub fn remove(name: &str) -> PyResult<()> {
        validate_name(name)?;
        PlatformHandle::remove(name).map_err(PyErr::from)
    }

    /// Maps the entire segment and returns a [`MappedView`].
    ///
    /// When called on a creator handle, the
    /// [`ShmRwLock`](crate::rwlock::ShmRwLock) header is initialised to the
    /// unlocked state.
    ///
    /// # Errors
    ///
    /// - [`PyValueError`] if the segment is too small for the rwlock header.
    /// - [`PyOSError`] on mapping failure.
    pub fn map(&self) -> PyResult<MappedView> {
        let ptr = self.handle.map().map_err(PyErr::from)?;
        MappedView::new(ptr, self.handle.size(), self.is_creator)
    }

    /// Returns the segment name.
    pub fn name(&self) -> &str {
        self.handle.name()
    }

    /// Returns the total segment size in bytes.
    pub fn size(&self) -> usize {
        self.handle.size()
    }

    fn __repr__(&self) -> String {
        format!(
            "SharedMemory(name='{}', size={}, role={})",
            self.handle.name(),
            self.handle.size(),
            if self.is_creator { "creator" } else { "opener" },
        )
    }
}

/// Validates POSIX-style shared memory names.
fn validate_name(name: &str) -> PyResult<()> {
    if !name.starts_with('/') {
        return Err(ShmError::InvalidArg(
            "name must start with '/' (e.g. \"/my_segment\")".into()
        ).into());
    }
    if name.len() < 2 {
        return Err(ShmError::InvalidArg(
            "name must have at least one character after '/'".into()
        ).into());
    }
    Ok(())
}

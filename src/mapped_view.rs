use pyo3::prelude::*;
use pyo3::types::PyBytes;
use std::ptr::NonNull;

use crate::error::ShmError;
use crate::rwlock::{ShmRwLock, ReadGuard, WriteGuard, RWLOCK_SIZE};

/// A view into a shared memory segment with integrated cross-process rwlock.
///
/// # Memory layout
///
/// The first [`RWLOCK_SIZE`] bytes of the mapped region are reserved for a
/// [`ShmRwLock`] header. All user-facing `offset` arguments are relative to
/// the start of the **data area**:
///
/// ```text
/// [ ShmRwLock (4 B) | user data (size - 4 B) ]
///   ^offset 0         ^all public offsets start here
/// ```
///
/// # Locking
///
/// [`read`] / [`read_range`] acquire a shared read lock for their duration.
/// [`write`] / [`zero`] acquire an exclusive write lock. Both are released
/// automatically on drop via RAII guards — no explicit unlock is needed.
///
/// # Cross-process safety
///
/// The [`ShmRwLock`] state lives inside the shared memory itself. Any process
/// that maps the same segment shares the same lock state automatically.
///
/// # Lifetime
///
/// `MappedView` holds a raw pointer into memory owned by the parent
/// [`SharedMemory`] handle. The [`SharedMemory`] must outlive all views
/// derived from it. Unlike the previous POSIX implementation, **no `munmap`
/// is called on drop** — memory lifetime is managed by `libafl_bolts`.
#[pyclass]
pub struct MappedView {
    /// Start of the region, including the rwlock header.
    ptr:  NonNull<u8>,
    /// Total region size in bytes, including the rwlock header.
    size: usize,
    /// Cross-process rwlock stored at `ptr + 0`.
    lock: ShmRwLock,
}

// SAFETY: `MappedView` holds a raw pointer into a shared memory region managed
// by libafl_bolts. The backing memory is valid for the lifetime of the parent
// `SharedMemory`. PyO3's GIL + RefCell interior mutability ensures that no two
// Rust references to the same `MappedView` coexist.
unsafe impl Send for MappedView {}

// SAFETY: All mutable access to the shared memory goes through `ShmRwLock`,
// which uses `AtomicI32` to coordinate readers and writers. Concurrent shared
// (`&`) access from multiple threads is therefore safe — the lock enforces the
// necessary exclusion internally.
unsafe impl Sync for MappedView {}

#[pymethods]
impl MappedView {
    /// Reads the entire data area under the read lock and returns it as
    /// [`bytes`].
    pub fn read<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        let _guard = ReadGuard::new(&self.lock);
        let slice = unsafe {
            std::slice::from_raw_parts(self.data_ptr(), self.data_size())
        };
        PyBytes::new(py, slice)
    }

    /// Reads `length` bytes starting at `offset` (relative to the data area)
    /// under the read lock.
    ///
    /// # Errors
    ///
    /// Returns [`PyValueError`] if `length == 0` or
    /// `offset + length > data_size`.
    pub fn read_range<'py>(
        &self,
        py: Python<'py>,
        offset: usize,
        length: usize,
    ) -> PyResult<Bound<'py, PyBytes>> {
        self.check_bounds(offset, length)?;
        let _guard = ReadGuard::new(&self.lock);
        let slice = unsafe {
            std::slice::from_raw_parts(self.data_ptr().add(offset), length)
        };
        Ok(PyBytes::new(py, slice))
    }

    /// Writes `data` into the data area starting at `offset` under the write
    /// lock.
    ///
    /// # Errors
    ///
    /// Returns [`PyValueError`] if `data` is empty or
    /// `offset + len(data) > data_size`.
    pub fn write(&self, offset: usize, data: &[u8]) -> PyResult<()> {
        self.check_bounds(offset, data.len())?;
        let _guard = WriteGuard::new(&self.lock);
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                self.data_ptr().add(offset),
                data.len(),
            );
        }
        Ok(())
    }

    /// Zeroes the entire data area under the write lock.
    pub fn zero(&self) {
        let _guard = WriteGuard::new(&self.lock);
        unsafe {
            std::ptr::write_bytes(self.data_ptr(), 0, self.data_size());
        }
    }

    /// Returns the number of processes/threads currently holding the read lock.
    pub fn reader_count(&self) -> i32 {
        self.lock.reader_count()
    }

    /// Returns `true` if a writer currently holds the lock.
    pub fn is_write_locked(&self) -> bool {
        self.lock.is_write_locked()
    }

    /// Returns the usable data area size in bytes (total size minus the
    /// [`RWLOCK_SIZE`] header).
    pub fn size(&self) -> usize {
        self.data_size()
    }

    fn __repr__(&self) -> String {
        format!(
            "MappedView(data_size={}, readers={}, write_locked={})",
            self.data_size(),
            self.lock.reader_count(),
            self.lock.is_write_locked(),
        )
    }
}

impl MappedView {
    /// Constructs a `MappedView` from a raw pointer into a libafl_bolts-managed
    /// shared memory region.
    ///
    /// Called exclusively by [`SharedMemory::map`](crate::shared_memory::SharedMemory::map).
    ///
    /// - `ptr`        — start of the region (must include the rwlock header).
    /// - `size`       — total size; must be greater than [`RWLOCK_SIZE`].
    /// - `is_creator` — when `true`, initialises the rwlock to the unlocked
    ///   state. Must be `true` for exactly one process.
    ///
    /// # Errors
    ///
    /// Returns [`PyValueError`] if `size <= RWLOCK_SIZE`.
    pub(crate) fn new(ptr: NonNull<u8>, size: usize, is_creator: bool) -> PyResult<Self> {
        if size <= RWLOCK_SIZE {
            return Err(ShmError::InvalidArg(format!(
                "segment size must be > {RWLOCK_SIZE} bytes (rwlock header)"
            )).into());
        }
        // SAFETY: `ptr` is a valid, aligned pointer into a live shmem region,
        // as guaranteed by the caller.
        let lock = unsafe { ShmRwLock::from_ptr(ptr) };
        if is_creator {
            lock.init();
        }
        Ok(MappedView { ptr, size, lock })
    }

    /// Returns a pointer to the start of the user data area (after the header).
    fn data_ptr(&self) -> *mut u8 {
        // SAFETY: `RWLOCK_SIZE < size` is enforced in `new`.
        unsafe { self.ptr.as_ptr().add(RWLOCK_SIZE) }
    }

    /// Returns the size of the user data area in bytes.
    fn data_size(&self) -> usize {
        self.size - RWLOCK_SIZE
    }

    /// Validates that `[offset, offset + length)` lies within the data area.
    fn check_bounds(&self, offset: usize, length: usize) -> PyResult<()> {
        if length == 0 {
            return Err(ShmError::InvalidArg("length cannot be zero".into()).into());
        }
        if offset.saturating_add(length) > self.data_size() {
            return Err(ShmError::InvalidArg(format!(
                "out of bounds: offset={offset} + length={length} > data_size={}",
                self.data_size()
            )).into());
        }
        Ok(())
    }
}

// No Drop impl needed: memory lifetime is managed by the parent SharedMemory /
// libafl_bolts, not by MappedView itself.

impl Drop for MappedView {
    /// Unmaps the memory region via the platform-specific unmap call.
    fn drop(&mut self) {
        // SAFETY: `ptr` and `size` come from a successful `PlatformHandle::map`
        // call and have not been unmapped yet.
        unsafe {
            crate::platform::PlatformHandle::unmap(self.ptr, self.size);
        }
    }
}

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use std::ptr::NonNull;

use pyo3::types::PyList;

use crate::error::ShmError;
use crate::mixed::{pack_mixed, unpack_mixed_counted, unpack_one};
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
    ptr:    NonNull<u8>,
    /// Total region size in bytes, including the rwlock header.
    size:   usize,
    /// Cross-process rwlock stored at `ptr + 0`.
    lock:   ShmRwLock,
    /// Auto-advance cursor for sequential writes (relative to data area).
    cursor: std::sync::atomic::AtomicUsize,
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
    pub fn read_all<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
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

    /// Writes `data` into the data area under the write lock.
    ///
    /// - If `offset` is given, writes at that position and **does not** advance
    ///   the cursor.
    /// - If `offset` is omitted (`None`), writes at the current cursor position
    ///   and advances the cursor by the number of bytes written.
    ///
    /// Accepted types: `bytes`, `bytearray`, `str` (UTF-8), `int` (i64),
    /// `float` (f64), `bool` (1 byte).
    ///
    /// # Errors
    ///
    /// Returns [`PyValueError`] if the write would exceed the data area.
    #[pyo3(signature = (data, offset=None))]
    pub fn write(&self, data: &Bound<'_, PyAny>, offset: Option<usize>) -> PyResult<()> {
        use std::sync::atomic::Ordering;

        let mut bytes: Vec<u8> = Vec::new();
        crate::mixed::pack_value(data, &mut bytes)?;

        let pos = match offset {
            Some(o) => o,
            None    => self.cursor.load(Ordering::Relaxed),
        };

        self.check_bounds(pos, bytes.len())?;
        let _guard = WriteGuard::new(&self.lock);
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.data_ptr().add(pos),
                bytes.len(),
            );
        }

        // Advance cursor only when offset was not given explicitly.
        if offset.is_none() {
            self.cursor.fetch_add(bytes.len(), Ordering::Relaxed);
        }

        Ok(())
    }

    /// Resets the auto-advance cursor to `pos` (default 0).
    #[pyo3(signature = (pos=0))]
    pub fn seek(&self, pos: usize) -> PyResult<()> {
        if pos > self.data_size() {
            return Err(ShmError::InvalidArg(format!(
                "seek position {pos} exceeds data_size {}", self.data_size()
            )).into());
        }
        self.cursor.store(pos, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Returns the current cursor position.
    pub fn tell(&self) -> usize {
        self.cursor.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Reads one value from the cursor position and advances the cursor.
    ///
    /// Mirrors :meth:`write` — use the same type tag that corresponds to what
    /// was written.
    ///
    /// Supported tags: ``"bool"``, ``"int"`` / ``"i64"``, ``"float"`` /
    /// ``"f64"``, ``"f32"``, ``"i32"``, ``"u8"``, ``"u32"``, ``"u64"``,
    /// ``"str"``.
    ///
    /// # Example
    ///
    /// ```python
    /// view.seek()
    /// view.write("hello")
    /// view.write(42)
    /// view.write(True)
    ///
    /// view.seek()
    /// msg  = view.read_one("str")    # → "hello"
    /// num  = view.read_one("int")    # → 42
    /// flag = view.read_one("bool")   # → True
    /// ```
    pub fn read<'py>(&self, py: Python<'py>, tag: &str) -> PyResult<Py<PyAny>> {
        use std::sync::atomic::Ordering;

        let pos = self.cursor.load(Ordering::Relaxed);
        let _guard = ReadGuard::new(&self.lock);
        let (value, bytes_read) = unpack_one(
            py,
            unsafe { std::slice::from_raw_parts(self.data_ptr().add(pos), self.data_size() - pos) },
            tag,
        )?;
        self.cursor.fetch_add(bytes_read, Ordering::Relaxed);
        Ok(value)
    }

    /// Writes heterogeneous typed lists into the data area starting at `offset`.
    ///
    /// `items` must be a Python `list` of `(type_tag: str, values: list)` tuples.
    ///
    /// Supported tags: `"f32"`, `"f64"`, `"i32"`, `"i64"`, `"u8"`, `"u32"`,
    /// `"u64"`, `"bool"`.
    ///
    /// Data is packed sequentially in native byte order.
    ///
    /// # Example
    ///
    /// ```python
    /// view.write_mixed(0, [
    ///     ("f32",  [1.0, 2.5]),
    ///     ("i64",  [100, 200]),
    ///     ("bool", [True, False]),
    /// ])
    /// ```
    /// Reads bytes from `offset` and unpacks them according to `schema`.
    ///
    /// `schema` is a `list` of `(type_tag, count)` tuples — the mirror image
    /// of [`write_mixed`].
    ///
    /// Returns a `list` of `list`, one per schema entry.
    ///
    /// # Example
    ///
    /// ```python
    /// labels, scores, flags = view.read_mixed(0, [
    ///     ("str16", 3),
    ///     ("f32",   3),
    ///     ("bool",  3),
    /// ])
    /// ```
    /// If `offset` is omitted, reads from the current cursor position and advances it.
    #[pyo3(signature = (schema, offset=None))]
    pub fn read_mixed<'py>(
        &self,
        py: Python<'py>,
        schema: &Bound<'py, PyList>,
        offset: Option<usize>,
    ) -> PyResult<Bound<'py, PyList>> {
        use std::sync::atomic::Ordering;
        let pos = match offset {
            Some(o) => o,
            None    => self.cursor.load(Ordering::Relaxed),
        };
        // Pass the entire remaining data area to unpack_mixed so it can
        // handle variable-length types (e.g. str with length prefix).
        // unpack_mixed returns (result, bytes_consumed) so we can advance
        // the cursor correctly.
        let available = self.data_size().saturating_sub(pos);
        if available == 0 {
            return Err(crate::error::ShmError::InvalidArg(
                "read_mixed: no data available at current cursor position".into()
            ).into());
        }
        let _guard = ReadGuard::new(&self.lock);
        let data = unsafe {
            std::slice::from_raw_parts(self.data_ptr().add(pos), available)
        };
        let (result, bytes_read) = unpack_mixed_counted(py, data, schema)?;
        if offset.is_none() {
            self.cursor.fetch_add(bytes_read, Ordering::Relaxed);
        }
        Ok(result)
    }

    /// If `offset` is omitted, writes at the current cursor position and advances it.
    #[pyo3(signature = (items, offset=None))]
    pub fn write_mixed(&self, items: &Bound<'_, PyList>, offset: Option<usize>) -> PyResult<()> {
        use std::sync::atomic::Ordering;
        let bytes = pack_mixed(items)?;
        let pos = match offset {
            Some(o) => o,
            None    => self.cursor.load(Ordering::Relaxed),
        };
        self.check_bounds(pos, bytes.len())?;
        let _guard = WriteGuard::new(&self.lock);
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.data_ptr().add(pos),
                bytes.len(),
            );
        }
        if offset.is_none() {
            self.cursor.fetch_add(bytes.len(), Ordering::Relaxed);
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
        Ok(MappedView {
            ptr,
            size,
            lock,
            cursor: std::sync::atomic::AtomicUsize::new(0),
        })
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
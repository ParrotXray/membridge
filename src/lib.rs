//! # membridge
//!
//! A PyO3-based Python extension for **cross-platform, cross-process** shared
//! memory with integrated reader-writer locking.
//!
//! ## Platform support
//!
//! | Platform | Backing API |
//! |----------|-------------|
//! | Linux / macOS | `shm_open` + `ftruncate` + `mmap` (POSIX) |
//! | Windows       | `CreateFileMappingA` + `MapViewOfFile` (Win32) |
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │                 Python API                   │
//! │      SharedMemory            MappedView      │
//! └────────┬─────────────────────────┬───────────┘
//!          │                         │ read/write (rwlock protected)
//! ┌────────▼─────────────────────────▼───────────┐
//! │            Shared Memory Segment              │
//! │   [ ShmRwLock (4 B) | user data ... ]        │
//! └───────────────────────────────────────────────┘
//!          │
//! ┌────────▼─────────────────────────────────────┐
//! │           platform::PlatformHandle            │
//! │   unix::PlatformHandle  (shm_open + mmap)    │
//! │   win32::PlatformHandle (CreateFileMapping)  │
//! └───────────────────────────────────────────────┘
//! ```
//!
//! ## Quick start
//!
//! ```python
//! import membridge
//!
//! # Process A — creator (Unix or Windows)
//! mem  = membridge.SharedMemory.create("/demo", 4096)
//! view = mem.map()
//! view.write(0, b"hello")
//!
//! # Process B — opener
//! # Unix:    size is read from fstat, no need to pass it
//! # Windows: size must be supplied explicitly
//! mem  = membridge.SharedMemory.open("/demo")          # Unix
//! mem  = membridge.SharedMemory.open("/demo", 4096)    # Windows
//! view = mem.map()
//! assert view.read_range(0, 5) == b"hello"
//!
//! # Cleanup (Unix only — Windows cleans up automatically)
//! membridge.SharedMemory.remove("/demo")
//! ```

mod error;
mod mixed;
mod platform;
mod rwlock;
mod mapped_view;
mod shared_memory;

pub use error::ShmError;
pub use mapped_view::MappedView;
pub use shared_memory::SharedMemory;

use pyo3::prelude::*;

#[pymodule]
fn membridge(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<SharedMemory>()?;
    m.add_class::<MappedView>()?;
    Ok(())
}

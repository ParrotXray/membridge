use std::ptr::NonNull;

use nix::fcntl::OFlag;
use nix::sys::mman::{self, MapFlags, ProtFlags};
use nix::sys::stat::Mode;
use nix::unistd::ftruncate;

use crate::error::ShmError;

/// POSIX shared memory handle (`shm_open` + `mmap`).
pub struct PlatformHandle {
    fd:   std::os::unix::io::OwnedFd,
    size: usize,
    name: String,
}

impl PlatformHandle {
    /// Creates a new named segment via `shm_open(O_CREAT | O_RDWR)` +
    /// `ftruncate`. Unlinks the segment if `ftruncate` fails.
    pub fn create(name: &str, size: usize) -> Result<Self, ShmError> {
        let fd = mman::shm_open(
            name,
            OFlag::O_CREAT | OFlag::O_RDWR,
            Mode::from_bits_truncate(0o666),
        ).map_err(ShmError::from)?;

        ftruncate(&fd, size as i64).map_err(|e| {
            let _ = mman::shm_unlink(name);
            ShmError::from(e)
        })?;

        Ok(PlatformHandle { fd, size, name: name.to_string() })
    }

    /// Opens an existing named segment via `shm_open(O_RDWR)`. Uses `fstat`
    /// to read the segment size.
    pub fn open(name: &str) -> Result<Self, ShmError> {
        let fd = mman::shm_open(
            name,
            OFlag::O_RDWR,
            Mode::empty(),
        ).map_err(ShmError::from)?;

        let stat = nix::sys::stat::fstat(&fd).map_err(ShmError::from)?;
        let size = stat.st_size as usize;

        Ok(PlatformHandle { fd, size, name: name.to_string() })
    }

    /// Unlinks the named segment via `shm_unlink`.
    ///
    /// Existing mappings remain valid until their last holder drops them.
    pub fn remove(name: &str) -> Result<(), ShmError> {
        mman::shm_unlink(name).map_err(ShmError::from)
    }

    /// Maps the entire segment with `mmap(MAP_SHARED | PROT_READ | PROT_WRITE)`
    /// and returns a raw pointer to the mapped region.
    ///
    /// The caller is responsible for calling `munmap` when done.
    pub fn map(&self) -> Result<NonNull<u8>, ShmError> {
        let ptr = unsafe {
            mman::mmap(
                None,
                std::num::NonZeroUsize::new(self.size).unwrap(),
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED,
                &self.fd,
                0,
            ).map_err(ShmError::from)?
        };

        // mmap returns NonNull<c_void>; reinterpret as NonNull<u8>.
        Ok(ptr.cast::<u8>())
    }

    /// Unmaps a previously mapped region.
    ///
    /// # Safety
    ///
    /// `ptr` and `size` must come from a successful [`map`](Self::map) call
    /// on this handle and must not have been unmapped already.
    pub unsafe fn unmap(ptr: NonNull<u8>, size: usize) {
        let _ = mman::munmap(ptr.cast::<std::ffi::c_void>(), size);
    }

    /// Returns the total segment size in bytes.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Returns the segment name.
    pub fn name(&self) -> &str {
        &self.name
    }
}
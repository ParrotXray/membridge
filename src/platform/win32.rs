use std::ptr::NonNull;

use windows::core::PCSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Memory::{
    CreateFileMappingA, MapViewOfFile, UnmapViewOfFile,
    OpenFileMappingA, FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
    MEMORY_MAPPED_VIEW_ADDRESS,
};

use crate::error::ShmError;

/// Win32 shared memory handle (`CreateFileMappingA` + `MapViewOfFile`).
///
/// The segment name is stored with its original leading `'/'`; the slash is
/// stripped internally before passing to Win32 APIs.
pub struct PlatformHandle {
    handle: HANDLE,
    size:   usize,
    name:   String,
}

// SAFETY: `HANDLE` is a Win32 kernel object reference that is valid to move
// across threads as long as it is not used concurrently without synchronisation.
unsafe impl Send for PlatformHandle {}
unsafe impl Sync for PlatformHandle {}

impl PlatformHandle {
    /// Creates a new named segment via `CreateFileMappingA`.
    pub fn create(name: &str, size: usize) -> Result<Self, ShmError> {
        let win_name = to_win32_name(name);
        let c_name = std::ffi::CString::new(win_name)
            .map_err(|e| ShmError::InvalidArg(e.to_string()))?;

        let high = (size >> 32) as u32;
        let low  = (size & 0xFFFF_FFFF) as u32;

        // SAFETY: all arguments are valid; c_name lives for the duration of
        // the call.
        let handle = unsafe {
            CreateFileMappingA(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                high,
                low,
                PCSTR(c_name.as_ptr() as *const u8),
            ).map_err(|e: windows::core::Error| ShmError::Os(e.to_string()))?
        };

        Ok(PlatformHandle { handle, size, name: name.to_string() })
    }

    /// Opens an existing named segment via `OpenFileMappingA`.
    ///
    /// `size` must match the value used at creation time — Win32 does not
    /// expose the segment size through the mapping handle.
    pub fn open(name: &str, size: usize) -> Result<Self, ShmError> {
        let win_name = to_win32_name(name);
        let c_name = std::ffi::CString::new(win_name)
            .map_err(|e| ShmError::InvalidArg(e.to_string()))?;

        // SAFETY: c_name is a valid null-terminated string.
        let handle = unsafe {
            OpenFileMappingA(
                FILE_MAP_ALL_ACCESS.0,
                false,
                PCSTR(c_name.as_ptr() as *const u8),
            ).map_err(|e: windows::core::Error| ShmError::Os(e.to_string()))?
        };

        Ok(PlatformHandle { handle, size, name: name.to_string() })
    }

    /// No-op on Windows — the kernel object is reference-counted and freed
    /// automatically when all handles are closed.
    pub fn remove(_name: &str) -> Result<(), ShmError> {
        Ok(())
    }

    /// Maps the segment via `MapViewOfFile` and returns a raw pointer.
    pub fn map(&self) -> Result<NonNull<u8>, ShmError> {
        // SAFETY: `self.handle` is a valid open mapping handle.
        let addr = unsafe {
            MapViewOfFile(self.handle, FILE_MAP_ALL_ACCESS, 0, 0, self.size)
        };

        if addr.Value.is_null() {
            let err = windows::core::Error::from_thread();
            return Err(ShmError::Os(err.to_string()));
        }

        NonNull::new(addr.Value as *mut u8)
            .ok_or_else(|| ShmError::Os("MapViewOfFile returned null".into()))
    }

    /// Unmaps a previously mapped region via `UnmapViewOfFile`.
    ///
    /// # Safety
    ///
    /// `ptr` must come from a successful [`map`](Self::map) call on this
    /// handle and must not have been unmapped already.
    pub unsafe fn unmap(ptr: NonNull<u8>, _size: usize) {
        let addr = MEMORY_MAPPED_VIEW_ADDRESS { Value: ptr.as_ptr() as *mut _ };
        // SAFETY: caller guarantees ptr is a valid mapped address.
        unsafe { let _ = UnmapViewOfFile(addr); }
    }

    /// Returns the total segment size in bytes.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Returns the segment name (with the original leading `'/'`).
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for PlatformHandle {
    fn drop(&mut self) {
        // SAFETY: `handle` is a valid open Win32 HANDLE that has not been
        // closed yet.
        unsafe { let _ = CloseHandle(self.handle); }
    }
}

/// Strips the leading `'/'` from a POSIX-style name for Win32 APIs.
///
/// `"/my_segment"` → `"my_segment"`
fn to_win32_name(name: &str) -> String {
    name.trim_start_matches('/').to_string()
}

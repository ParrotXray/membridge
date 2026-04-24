/// Platform-specific shared memory handle.
///
/// On Unix this wraps a POSIX `shm_open` file descriptor.
/// On Windows this wraps a `CreateFileMapping` kernel object handle.
///
/// Both implementations expose the same interface:
/// - [`PlatformHandle::create`] — create and size a new segment
/// - [`PlatformHandle::open`]   — attach to an existing segment by name
/// - [`PlatformHandle::remove`] — unlink / no-op depending on platform
/// - [`PlatformHandle::map`]    — map the segment, return a raw pointer + size
/// - [`PlatformHandle::size`]   — total segment size in bytes

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::PlatformHandle;

#[cfg(windows)]
mod win32;
#[cfg(windows)]
pub use win32::PlatformHandle;

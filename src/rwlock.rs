use std::sync::atomic::{AtomicI32, Ordering};
use std::ptr::NonNull;

/// Size of the [`ShmRwLock`] header in bytes.
///
/// Callers must reserve this many bytes at the start of a mapped region before
/// any user data. [`MappedView`](crate::mapped_view::MappedView) handles this
/// offset automatically.
pub const RWLOCK_SIZE: usize = 4; // size_of::<AtomicI32>()

/// A cross-process reader-writer lock backed by a single [`AtomicI32`] stored
/// inside shared memory.
///
/// # State encoding
///
/// | Value | Meaning |
/// |-------|---------|
/// | `> 0` | `N` readers currently hold the read lock |
/// | `0`   | Lock is free |
/// | `-1`  | One writer holds the write lock |
///
/// # Memory layout
///
/// ```text
/// [ AtomicI32 (4 bytes) | user data ... ]
///   ^
///   ShmRwLock lives here
/// ```
///
/// # Safety
///
/// `ShmRwLock` does **not** own the memory it points to. The pointed-to region
/// must remain valid and mapped for the entire lifetime of the lock.
pub struct ShmRwLock {
    state: NonNull<AtomicI32>,
}

unsafe impl Send for ShmRwLock {}
unsafe impl Sync for ShmRwLock {}

impl ShmRwLock {
    /// Constructs a [`ShmRwLock`] from a raw pointer into shared memory.
    ///
    /// # Safety
    ///
    /// - `ptr` must point to a 4-byte-aligned location inside a valid mapped
    ///   shared memory region.
    /// - The mapped region must outlive this [`ShmRwLock`] instance.
    /// - Only the **creator** process should call [`init`](Self::init) before
    ///   any other process accesses the lock.
    pub unsafe fn from_ptr(ptr: NonNull<u8>) -> Self {
        ShmRwLock {
            state: ptr.cast::<AtomicI32>(),
        }
    }

    /// Initialises the lock state to zero (unlocked).
    ///
    /// Must be called exactly once by the process that created the shared
    /// memory segment, before any reader or writer acquires the lock.
    pub fn init(&self) {
        self.atomic().store(0, Ordering::SeqCst);
    }

    /// Acquires the read lock, spin-waiting until no writer holds the lock.
    ///
    /// Multiple readers may hold the lock concurrently.
    pub fn read_lock(&self) {
        let atomic = self.atomic();
        loop {
            let cur = atomic.load(Ordering::Acquire);
            if cur >= 0 {
                // Attempt to increment the reader count atomically.
                if atomic.compare_exchange(
                    cur,
                    cur + 1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ).is_ok() {
                    return;
                }
            }
            // A writer holds the lock or a concurrent CAS failed; yield the CPU.
            std::hint::spin_loop();
        }
    }

    /// Releases the read lock by decrementing the reader count.
    pub fn read_unlock(&self) {
        self.atomic().fetch_sub(1, Ordering::Release);
    }

    /// Acquires the write lock, spin-waiting until the state is exactly zero
    /// (no readers and no other writer).
    pub fn write_lock(&self) {
        let atomic = self.atomic();
        loop {
            if atomic.compare_exchange(
                0,
                -1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ).is_ok() {
                return;
            }
            std::hint::spin_loop();
        }
    }

    /// Releases the write lock by resetting the state to zero.
    pub fn write_unlock(&self) {
        self.atomic().store(0, Ordering::Release);
    }

    /// Returns the number of readers currently holding the lock, or `0` if a
    /// writer holds it.
    pub fn reader_count(&self) -> i32 {
        let v = self.atomic().load(Ordering::Acquire);
        if v > 0 { v } else { 0 }
    }

    /// Returns `true` if a writer currently holds the lock.
    pub fn is_write_locked(&self) -> bool {
        self.atomic().load(Ordering::Acquire) == -1
    }

    fn atomic(&self) -> &AtomicI32 {
        // SAFETY: `state` points to a valid `AtomicI32` inside shared memory,
        // as guaranteed by the caller of `from_ptr`.
        unsafe { self.state.as_ref() }
    }
}

/// RAII guard that holds the read lock for the duration of its lifetime.
///
/// The lock is released automatically when this guard is dropped.
pub struct ReadGuard<'a> {
    lock: &'a ShmRwLock,
}

impl<'a> ReadGuard<'a> {
    /// Acquires the read lock and returns the guard.
    pub fn new(lock: &'a ShmRwLock) -> Self {
        lock.read_lock();
        ReadGuard { lock }
    }
}

impl Drop for ReadGuard<'_> {
    fn drop(&mut self) {
        self.lock.read_unlock();
    }
}

/// RAII guard that holds the write lock for the duration of its lifetime.
///
/// The lock is released automatically when this guard is dropped.
pub struct WriteGuard<'a> {
    lock: &'a ShmRwLock,
}

impl<'a> WriteGuard<'a> {
    /// Acquires the write lock and returns the guard.
    pub fn new(lock: &'a ShmRwLock) -> Self {
        lock.write_lock();
        WriteGuard { lock }
    }
}

impl Drop for WriteGuard<'_> {
    fn drop(&mut self) {
        self.lock.write_unlock();
    }
}

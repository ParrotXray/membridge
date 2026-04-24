use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};

use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::error::ShmError;
use crate::mixed::{pack_value, unpack_one, unpack_mixed};

// ═══════════════════════════════════════════════════════════════
//  Memory layout
//
//  [ head: AtomicUsize (8 B)
//  | tail: AtomicUsize (8 B)
//  | capacity: usize   (8 B)   ← written once at init, then read-only
//  | data: [u8; capacity]  ]
//
//  head — index of the next slot the consumer will read
//  tail — index of the next slot the producer will write
//
//  Invariant: the buffer is full when (tail - head) == capacity.
//  Both indices grow monotonically and are masked with capacity - 1
//  only when accessing the data array (power-of-two capacity required).
// ═══════════════════════════════════════════════════════════════

/// Size of the ring buffer header in bytes.
pub const SPSC_HEADER_SIZE: usize = 24; // head(8) + tail(8) + capacity(8)

/// A lock-free single-producer / single-consumer ring buffer stored entirely
/// inside a shared memory region.
///
/// The buffer operates on variable-length **messages**. Each message is
/// prefixed with a 4-byte (`u32`) length field so the consumer knows how many
/// bytes to read.
///
/// # Layout
///
/// ```text
/// [ head (8 B) | tail (8 B) | capacity (8 B) | data (capacity B) ]
/// ```
///
/// # Cross-platform
///
/// Uses only `AtomicUsize` and raw memory — no OS primitives. Works
/// identically on Windows and Unix.
///
/// # Safety
///
/// Exactly **one** producer and **one** consumer must access this ring buffer
/// at any time. Violating this contract leads to data races.
#[pyclass]
pub struct SpscRingBuffer {
    ptr:      NonNull<u8>,
    /// Total mapped size including the header.
    map_size: usize,
}

unsafe impl Send for SpscRingBuffer {}
unsafe impl Sync for SpscRingBuffer {}

#[pymethods]
impl SpscRingBuffer {
    /// Writes a message into the ring buffer.
    ///
    /// The message is prefixed with a 4-byte length so the consumer can read
    /// it back as a complete unit.
    ///
    /// Accepted types: `bytes`, `bytearray`, `str` (UTF-8).
    ///
    /// :returns: ``True`` if the message was written, ``False`` if the buffer
    ///           does not have enough free space (back-pressure signal).
    pub fn push(&self, data: &Bound<'_, PyAny>) -> PyResult<bool> {
        use pyo3::types::PyList;

        // Accept a nested list → pack each element in order.
        // Otherwise pack the single value directly.
        let mut payload: Vec<u8> = Vec::new();
        if let Ok(list) = data.cast::<PyList>() {
            for item in list.iter() {
                pack_value(&item, &mut payload)?;
            }
        } else {
            pack_value(data, &mut payload)?;
        }

        let frame_len = 4 + payload.len(); // u32 prefix + payload
        let cap = self.capacity();

        // Guard: a single message can never exceed the total capacity.
        if frame_len > cap {
            return Err(crate::error::ShmError::InvalidArg(format!(
                "message too large: frame_len={frame_len} > capacity={cap}                  (map_size={})",
                self.map_size
            )).into());
        }

        let head = self.head().load(Ordering::Acquire);
        let tail = self.tail().load(Ordering::Relaxed);
        let used = tail.wrapping_sub(head);

        if used + frame_len > cap {
            return Ok(false); // buffer full — caller should retry
        }

        // Write the 4-byte length prefix.
        let len_bytes = (payload.len() as u32).to_ne_bytes();
        self.write_bytes(tail, &len_bytes, cap);
        // Write the payload.
        self.write_bytes(tail + 4, &payload, cap);

        // Publish: advance tail so the consumer sees the new data.
        self.tail().store(tail.wrapping_add(frame_len), Ordering::Release);
        Ok(true)
    }

    /// Reads one message from the ring buffer.
    ///
    /// - If `tag` is ``None``, returns the raw ``bytes``.
    /// - If `tag` is given (e.g. ``"str"``, ``"int"``), unpacks the bytes and
    ///   returns the typed value.
    ///
    /// :returns: The value (or raw bytes), or ``None`` if the buffer is empty.
    #[pyo3(signature = (tag=None))]
    pub fn pop<'py>(&self, py: Python<'py>, tag: Option<&str>) -> PyResult<Option<Py<PyAny>>> {
        let head = self.head().load(Ordering::Relaxed);
        let tail = self.tail().load(Ordering::Acquire);

        if head == tail {
            return Ok(None); // buffer empty
        }

        let cap = self.capacity();

        // Read the 4-byte length prefix.
        let mut len_bytes = [0u8; 4];
        self.read_bytes(head, &mut len_bytes, cap);
        let payload_len = u32::from_ne_bytes(len_bytes) as usize;

        // Read the payload.
        let mut payload = vec![0u8; payload_len];
        self.read_bytes(head + 4, &mut payload, cap);

        // Consume: advance head.
        let frame_len = 4 + payload_len;
        self.head().store(head.wrapping_add(frame_len), Ordering::Release);

        match tag {
            None => Ok(Some(PyBytes::new(py, &payload).into_any().unbind())),
            Some(t) => {
                let (value, _) = unpack_one(py, &payload, t)?;
                Ok(Some(value))
            }
        }
    }

    /// Returns the number of bytes currently used in the ring buffer.
    pub fn used(&self) -> usize {
        let head = self.head().load(Ordering::Acquire);
        let tail = self.tail().load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }

    /// Returns the total data capacity in bytes (excluding the header).
    pub fn capacity(&self) -> usize {
        // SAFETY: capacity field is written once at init and never modified.
        unsafe {
            let ptr = self.ptr.as_ptr().add(16) as *const usize;
            ptr.read()
        }
    }

    /// Returns the number of free bytes remaining.
    pub fn free(&self) -> usize {
        self.capacity().saturating_sub(self.used())
    }

    /// Returns ``True`` if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        let head = self.head().load(Ordering::Acquire);
        let tail = self.tail().load(Ordering::Acquire);
        head == tail
    }

    /// Returns ``True`` if the buffer has no free space.
    pub fn is_full(&self) -> bool {
        self.used() >= self.capacity()
    }

    /// Pops one message and unpacks it according to `schema`.
    ///
    /// Mirrors :meth:`push` for list values written via :func:`write_mixed`.
    ///
    /// :returns: A list of unpacked values, or ``None`` if the buffer is empty.
    pub fn pop_mixed<'py>(
        &self,
        py: Python<'py>,
        schema: &Bound<'py, pyo3::types::PyList>,
    ) -> PyResult<Option<Bound<'py, pyo3::types::PyList>>> {
        let head = self.head().load(Ordering::Relaxed);
        let tail = self.tail().load(Ordering::Acquire);
        if head == tail {
            return Ok(None);
        }
        let cap = self.capacity();
        let mut len_bytes = [0u8; 4];
        self.read_bytes(head, &mut len_bytes, cap);
        let payload_len = u32::from_ne_bytes(len_bytes) as usize;
        let mut payload = vec![0u8; payload_len];
        self.read_bytes(head + 4, &mut payload, cap);
        self.head().store(head.wrapping_add(4 + payload_len), Ordering::Release);
        let result = unpack_mixed(py, &payload, schema)?;
        Ok(Some(result))
    }

    fn __repr__(&self) -> String {
        format!(
            "SpscRingBuffer(capacity={}, used={}, free={})",
            self.capacity(),
            self.used(),
            self.free(),
        )
    }
}

impl SpscRingBuffer {
    /// Constructs a ring buffer view over `ptr`.
    ///
    /// - `map_size`   — total mapped region size (header + data).
    /// - `is_creator` — when `true`, zeroes head/tail and writes capacity.
    pub(crate) fn new(ptr: NonNull<u8>, map_size: usize, is_creator: bool) -> PyResult<()> {
        if map_size <= SPSC_HEADER_SIZE {
            return Err(ShmError::InvalidArg(format!(
                "map_size must be > {SPSC_HEADER_SIZE} bytes (spsc header)"
            )).into());
        }
        if is_creator {
            let capacity = map_size - SPSC_HEADER_SIZE;
            // Verify power-of-two requirement.
            if !capacity.is_power_of_two() {
                return Err(ShmError::InvalidArg(format!(
                    "data capacity ({capacity}) must be a power of two; \
                     try {} or {}",
                    capacity.next_power_of_two() / 2,
                    capacity.next_power_of_two(),
                )).into());
            }
            // SAFETY: ptr points to a valid mapped region of size map_size.
            unsafe {
                (ptr.as_ptr() as *mut usize).write(0);        // head = 0
                (ptr.as_ptr().add(8) as *mut usize).write(0); // tail = 0
                (ptr.as_ptr().add(16) as *mut usize).write(capacity); // capacity
            }
        }
        Ok(())
    }

    pub(crate) fn from_ptr(ptr: NonNull<u8>, map_size: usize) -> Self {
        SpscRingBuffer { ptr, map_size }
    }

    fn head(&self) -> &AtomicUsize {
        // SAFETY: head is at offset 0, valid for the lifetime of the mapping.
        unsafe { &*(self.ptr.as_ptr() as *const AtomicUsize) }
    }

    fn tail(&self) -> &AtomicUsize {
        // SAFETY: tail is at offset 8, valid for the lifetime of the mapping.
        unsafe { &*(self.ptr.as_ptr().add(8) as *const AtomicUsize) }
    }

    fn data_ptr(&self) -> *mut u8 {
        // SAFETY: SPSC_HEADER_SIZE < map_size is enforced in new().
        unsafe { self.ptr.as_ptr().add(SPSC_HEADER_SIZE) }
    }

    /// Writes `src` into the ring buffer data area starting at logical index
    /// `pos`, wrapping around at `cap`.
    fn write_bytes(&self, pos: usize, src: &[u8], cap: usize) {
        let mask = cap - 1; // cap is power-of-two
        for (i, &byte) in src.iter().enumerate() {
            let idx = (pos + i) & mask;
            // SAFETY: idx < cap, data area is valid.
            unsafe { self.data_ptr().add(idx).write(byte); }
        }
    }

    /// Reads `dst.len()` bytes from the ring buffer data area starting at
    /// logical index `pos`, wrapping around at `cap`.
    fn read_bytes(&self, pos: usize, dst: &mut [u8], cap: usize) {
        let mask = cap - 1;
        for (i, slot) in dst.iter_mut().enumerate() {
            let idx = (pos + i) & mask;
            // SAFETY: idx < cap, data area is valid.
            *slot = unsafe { self.data_ptr().add(idx).read() };
        }
    }
}
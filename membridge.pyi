class SharedMemory:
    """
    A handle to a named cross-platform shared memory segment.

    Platform behaviour:

    - **Unix**    — backed by ``shm_open`` + ``mmap``. The segment persists
      until :meth:`remove` is called, even after all handles are closed.
    - **Windows** — backed by ``CreateFileMappingA``. The kernel object is
      reference-counted and freed automatically when all handles are closed;
      :meth:`remove` is a no-op.

    Names must start with ``'/'`` and contain at least one additional
    character, e.g. ``"/my_segment"``.
    """

    def __init__(self) -> None: ...  # not constructable directly; use create / open

    @staticmethod
    def create(name: str, size: int) -> "SharedMemory":
        """
        Create a new named shared memory segment of ``size`` bytes.

        The first process to call this becomes the *creator*. When the
        returned handle is used to call :meth:`map`, the
        :class:`ShmRwLock` header is initialised to the unlocked state.

        :param name: Segment name, must start with ``'/'``.
        :param size: Total size in bytes. Must be greater than 0 and greater
                     than 4 (the rwlock header occupies the first 4 bytes).
        :raises ValueError: If ``size == 0`` or ``name`` is invalid.
        :raises OSError: On OS-level failure.
        """
        ...

    @staticmethod
    def open(name: str, size: int | None = None) -> "SharedMemory":
        """
        Open an existing named shared memory segment.

        On **Unix** ``size`` is read automatically from ``fstat`` and may
        be omitted. On **Windows** ``size`` must be supplied explicitly and
        must match the value used at creation time (Win32 does not expose
        the segment size through the mapping handle).

        :param name: Segment name, must start with ``'/'``.
        :param size: Required on Windows; ignored on Unix.
        :raises ValueError: If ``name`` is invalid, or ``size`` is omitted
                            on Windows.
        :raises OSError: If the segment does not exist or the call fails.
        """
        ...

    @staticmethod
    def remove(name: str) -> None:
        """
        Remove the named segment (Unix: ``shm_unlink``; Windows: no-op).

        On Unix, processes that already have the segment mapped may continue
        to use their :class:`MappedView` instances; the kernel reclaims the
        backing memory only after the last mapping is released.

        :param name: Segment name, must start with ``'/'``.
        :raises ValueError: If ``name`` is invalid.
        :raises OSError: On Unix syscall failure.
        """
        ...

    def map(self) -> "MappedView":
        """
        Map the entire segment into the calling process's address space.

        Returns a :class:`MappedView` backed by the shared memory region.
        Writes are immediately visible to all other processes that have
        mapped the same segment.

        On Unix, ``munmap`` is called automatically when the view is
        garbage-collected. On Windows, ``UnmapViewOfFile`` is called instead.

        :raises ValueError: If the segment is too small to hold the 4-byte
                            rwlock header.
        :raises OSError: On mapping failure.
        """
        ...

    def name(self) -> str:
        """Return the segment name (including the leading ``'/'``)."""
        ...

    def size(self) -> int:
        """Return the total segment size in bytes."""
        ...

    def __repr__(self) -> str: ...


class MappedView:
    """
    A view into a shared memory segment with an integrated cross-process
    reader-writer lock.

    Memory layout::

        [ ShmRwLock (4 B) | user data (size - 4 B) ]
          offset 0          all public offsets start here

    All ``offset`` arguments are relative to the start of the **data area**
    (i.e. after the 4-byte rwlock header).

    **Locking**

    - :meth:`read` and :meth:`read_range` acquire the *shared read lock* for
      their duration — multiple readers may proceed concurrently.
    - :meth:`write` and :meth:`zero` acquire the *exclusive write lock* —
      all readers and other writers are blocked until the call returns.

    Locks are released automatically; no explicit unlock is needed.

    **Cross-process safety**

    The lock state lives inside the shared memory itself. Any process that
    maps the same segment at offset 0 shares the same lock automatically.

    **Drop behaviour**

    The mapped region is unmapped (``munmap`` / ``UnmapViewOfFile``)
    automatically when this object is garbage-collected.
    """

    def read(self) -> bytes:
        """
        Read the entire data area under the read lock.

        :returns: A copy of all valid bytes as an immutable ``bytes`` object.
        """
        ...

    def read_range(self, offset: int, length: int) -> bytes:
        """
        Read ``length`` bytes starting at ``offset`` under the read lock.

        :param offset: Byte offset into the data area (0-based).
        :param length: Number of bytes to read. Must be greater than 0.
        :returns: A copy of the requested bytes as an immutable ``bytes`` object.
        :raises ValueError: If ``length == 0`` or ``offset + length`` exceeds
                            the data area size.
        """
        ...

    def write(self, offset: int, data: bytes | bytearray) -> None:
        """
        Write ``data`` into the data area starting at ``offset`` under the
        write lock.

        :param offset: Byte offset into the data area (0-based).
        :param data:   Bytes to write.
        :raises ValueError: If ``data`` is empty or ``offset + len(data)``
                            exceeds the data area size.
        """
        ...

    def zero(self) -> None:
        """Zero the entire data area under the write lock."""
        ...

    def reader_count(self) -> int:
        """
        Return the number of processes/threads currently holding the read lock.

        Returns ``0`` if a writer holds the lock.
        """
        ...

    def is_write_locked(self) -> bool:
        """Return ``True`` if a writer currently holds the lock."""
        ...

    def size(self) -> int:
        """
        Return the usable data area size in bytes.

        Equal to the total segment size minus 4 (the rwlock header).
        """
        ...

    def __repr__(self) -> str: ...

from typing import Any

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

    @staticmethod
    def create(name: str, size: int) -> "SharedMemory":
        """
        Create a new named shared memory segment of ``size`` bytes.

        :param name: Segment name, must start with ``'/'``.
        :param size: Total size in bytes. Must be > 4 (rwlock header = 4 bytes).
        :raises ValueError: If ``size == 0`` or ``name`` is invalid.
        :raises OSError: On OS-level failure.
        """
        ...

    @staticmethod
    def open(name: str, size: int | None = None) -> "SharedMemory":
        """
        Open an existing named shared memory segment.

        On **Unix** ``size`` is read automatically via ``fstat`` and may be
        omitted. On **Windows** ``size`` must be supplied explicitly.

        :param name: Segment name, must start with ``'/'``.
        :param size: Required on Windows; ignored on Unix.
        :raises ValueError: If ``name`` is invalid or ``size`` is omitted on Windows.
        :raises OSError: If the segment does not exist or the call fails.
        """
        ...

    @staticmethod
    def remove(name: str) -> None:
        """
        Remove the named segment (Unix: ``shm_unlink``; Windows: no-op).

        :param name: Segment name, must start with ``'/'``.
        :raises ValueError: If ``name`` is invalid.
        :raises OSError: On Unix syscall failure.
        """
        ...

    def map(self) -> "MappedView":
        """
        Map the entire segment into the calling process's address space.

        On the creator side, the rwlock header is initialised to unlocked.

        :raises ValueError: If the segment is too small for the rwlock header.
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

    **Auto-advance cursor**

    :meth:`write` and :meth:`write_mixed` accept an optional ``offset``
    argument. When omitted, they write at the current cursor position and
    advance it automatically. Use :meth:`seek` and :meth:`tell` to control
    the cursor.

    **Accepted types for** :meth:`write`

    | Python type  | Stored as              | Bytes        |
    |--------------|------------------------|--------------|
    | ``bool``     | ``u8`` (0 or 1)        | 1            |
    | ``int``      | ``i64``                | 8            |
    | ``float``    | ``f64``                | 8            |
    | ``str``      | ``u32`` len + UTF-8    | 4 + len(str) |
    | ``bytes``    | raw bytes              | len          |
    | ``bytearray``| raw bytes              | len          |

    **Schema tags for** :meth:`read_mixed`

    ``"bool"``, ``"int"`` / ``"i64"``, ``"float"`` / ``"f64"``,
    ``"f32"``, ``"i32"``, ``"u8"``, ``"u32"``, ``"u64"``, ``"str"``
    """

    def read(self) -> bytes:
        """Read the entire data area under the read lock."""
        ...

    def read_range(self, offset: int, length: int) -> bytes:
        """
        Read ``length`` bytes at ``offset`` under the read lock.

        :raises ValueError: If ``length == 0`` or range exceeds data area.
        """
        ...

    def write(
        self,
        data: bool | int | float | str | bytes | bytearray,
        offset: int | None = None,
    ) -> None:
        """
        Write ``data`` under the write lock.

        If ``offset`` is omitted, writes at the cursor and advances it.
        If ``offset`` is given, writes there without moving the cursor.

        :raises ValueError: If the write would exceed the data area.
        """
        ...

    def write_mixed(
        self,
        items: list[Any],
        offset: int | None = None,
    ) -> None:
        """
        Pack and write a heterogeneous list under the write lock.

        Each element of ``items`` can be:

        - A plain value (``bool``, ``int``, ``float``, ``str``, ``bytes``) —
          packed directly using the same rules as :meth:`write`.
        - A nested ``list`` — each element is packed in order.

        If ``offset`` is omitted, writes at the cursor and advances it.

        Example::

            view.write_mixed([
                ["Normal", "DDoS", "DoS"],
                [0.92, 0.87, 0.95],
                [True, False, True],
                42,
            ])
        """
        ...

    def read_mixed(
        self,
        schema: list[tuple[str, int]],
        offset: int | None = None,
    ) -> list[Any]:
        """
        Read and unpack data according to ``schema`` under the read lock.

        ``schema`` is a list of ``(type_tag, count)`` tuples. When
        ``count == 1`` the corresponding element is a scalar; otherwise a
        list.

        If ``offset`` is omitted, reads from the cursor and advances it.

        Example::

            msg, count, labels, scores = view.read_mixed([
                ("str",   1),
                ("int",   1),
                ("str",   3),
                ("float", 3),
            ])

        :raises ValueError: If the data area would be exceeded.
        """
        ...

    def zero(self) -> None:
        """Zero the entire data area under the write lock."""
        ...

    def seek(self, pos: int = 0) -> None:
        """
        Set the auto-advance cursor to ``pos`` (default 0).

        :raises ValueError: If ``pos`` exceeds the data area size.
        """
        ...

    def tell(self) -> int:
        """Return the current cursor position."""
        ...

    def reader_count(self) -> int:
        """Return the number of active readers (0 if a writer holds the lock)."""
        ...

    def is_write_locked(self) -> bool:
        """Return ``True`` if a writer currently holds the lock."""
        ...

    def size(self) -> int:
        """Return the usable data area size (total size minus 4-byte rwlock header)."""
        ...

    def __repr__(self) -> str: ...

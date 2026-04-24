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

    def spsc(self) -> "SpscRingBuffer":
        """
        Create a :class:`SpscRingBuffer` view over the entire segment.

        The segment size minus 24 bytes (header) must be a power of two.

        :raises ValueError: If the data capacity is not a power of two.
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

    def read_all(self) -> bytes:
        """Read the entire data area under the read lock."""
        ...

    def read(self, tag: str) -> bool | int | float | str | bytes:
        """
        Read one value at the cursor position and advance it.

        Mirrors :meth:`write` — use the same tag as when writing.

        Supported tags: ``"bool"``, ``"int"``, ``"float"``, ``"f32"``,
        ``"f64"``, ``"i32"``, ``"i64"``, ``"u8"``, ``"u32"``, ``"u64"``,
        ``"str"``.

        Example::

            view.seek()
            view.write("hello")
            view.write(42)
            view.write(True)

            view.seek()
            msg  = view.read("str")    # → "hello"
            num  = view.read("int")    # → 42
            flag = view.read("bool")   # → True
        """
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


class SpscRingBuffer:
    """
    A lock-free single-producer / single-consumer ring buffer stored inside
    shared memory.

    Each message is prefixed with a 4-byte length field so the consumer
    knows how many bytes to read back as a complete unit.

    **Memory layout**::

        [ head (8 B) | tail (8 B) | capacity (8 B) | data (capacity B) ]

    **Cross-platform**: uses only ``AtomicUsize`` and raw memory — no OS
    primitives. Works identically on Windows and Unix.

    **Constraint**: the data capacity (``size - 24``) must be a power of two.
    Use ``size = N + 24`` where N is a power of two (e.g. 1024, 4096, 65536).

    Obtain via :meth:`SharedMemory.spsc`.

    **Accepted types for** :meth:`push`

    | Python type  | Stored as              | Bytes        |
    |--------------|------------------------|--------------|
    | ``bool``     | ``u8`` (0 or 1)        | 1            |
    | ``int``      | ``i64``                | 8            |
    | ``float``    | ``f64``                | 8            |
    | ``str``      | ``u32`` len + UTF-8    | 4 + len(str) |
    | ``bytes``    | raw bytes              | len          |
    | ``bytearray``| raw bytes              | len          |
    | ``list``     | each element packed    | sum of above |
    """

    def push(
        self,
        data: bool | int | float | str | bytes | bytearray | list,
    ) -> bool:
        """
        Write one message into the ring buffer.

        :param data: Value to push. A ``list`` packs each element in order.
        :returns: ``True`` if written, ``False`` if the buffer is full
                  (back-pressure signal — retry later).
        :raises ValueError: If the packed message is larger than the total
                            buffer capacity (message can never fit).
        """
        ...

    def pop(self, tag: str | None = None) -> bool | int | float | str | bytes | None:
        """
        Read one message from the ring buffer.

        - If ``tag`` is ``None``, returns the raw ``bytes``.
        - If ``tag`` is given (e.g. ``"str"``, ``"int"``), unpacks the bytes
          and returns the typed value.

        Supported tags: ``"bool"``, ``"int"`` / ``"i64"``, ``"float"`` /
        ``"f64"``, ``"f32"``, ``"i32"``, ``"u8"``, ``"u32"``, ``"u64"``,
        ``"str"``.

        :returns: The value, or ``None`` if the buffer is empty.
        """
        ...

    def pop_mixed(
        self,
        schema: list[tuple[str, int]],
    ) -> list | None:
        """
        Read one message and unpack it according to ``schema``.

        Mirrors :meth:`push` for list values.

        :returns: A list of unpacked values, or ``None`` if the buffer is empty.

        Example::

            ring.push(["Normal", 0.92, True])
            label, score, flag = ring.pop_mixed([
                ("str",   1),
                ("float", 1),
                ("bool",  1),
            ])
        """
        ...

    def used(self) -> int:
        """Return the number of bytes currently used in the buffer."""
        ...

    def free(self) -> int:
        """Return the number of free bytes remaining."""
        ...

    def capacity(self) -> int:
        """Return the total data capacity in bytes (excluding the 24-byte header)."""
        ...

    def is_empty(self) -> bool:
        """Return ``True`` if the buffer contains no messages."""
        ...

    def is_full(self) -> bool:
        """Return ``True`` if the buffer has no free space."""
        ...

    def __repr__(self) -> str: ...

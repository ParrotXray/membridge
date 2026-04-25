# membridge

[![PyPI](https://img.shields.io/pypi/v/membridge)](https://pypi.org/project/membridge/)
[![Python](https://img.shields.io/pypi/pyversions/membridge)](https://pypi.org/project/membridge/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/ParrotXray/membridge/actions/workflows/ci.yml/badge.svg)](https://github.com/ParrotXray/membridge/actions)

Cross-platform, cross-process **shared memory** for Python — backed by Rust via [PyO3](https://pyo3.rs).

membridge provides two communication primitives over a named shared memory segment:

| Primitive | Use case |
|-----------|----------|
| `MappedView` | Sequential read / write with a built-in cross-process **reader-writer lock** |
| `SpscRingBuffer` | Lock-free **single-producer / single-consumer** message queue |

Both primitives work identically on Linux, macOS, and Windows.

---

## Platform support

| Platform | Shared memory backend |
|----------|-----------------------|
| Linux / macOS | `shm_open` + `ftruncate` + `mmap` (POSIX) |
| Windows | `CreateFileMappingA` + `MapViewOfFile` (Win32) |

---

## Installation

```bash
pip install membridge
```

Requires Python ≥ 3.10. Pre-built wheels are available for Linux (x86\_64, aarch64), macOS (universal2), and Windows (x86\_64).

---

## Quick start

### MappedView — sequential shared memory with rwlock

```python
# process A: producer
import membridge

mem  = membridge.SharedMemory.create("/demo", 4096)
view = mem.map()

view.write("Hello from A!")   # str → length-prefixed UTF-8
view.write(42)                # int → i64
view.write(3.14)              # float → f64
view.write(True)              # bool → u8
```

```python
# process B: consumer
import membridge

mem  = membridge.SharedMemory.open("/demo")   # size auto-detected on Unix
view = mem.map()

msg   = view.read("str")    # → "Hello from A!"
count = view.read("int")    # → 42
ratio = view.read("float")  # → 3.14
flag  = view.read("bool")   # → True

membridge.SharedMemory.remove("/demo")
```

### SpscRingBuffer — lock-free message queue

```python
# producer
import membridge

mem  = membridge.SharedMemory.create("/spsc", 1024 + 24)
ring = mem.spsc()

ring.push("msg-0")
ring.push(["Hello", 0.92, True])   # heterogeneous list in one message
ring.push("__EOF__")
```

```python
# consumer
import time, membridge

mem  = membridge.SharedMemory.open("/spsc", 1024 + 24)
ring = mem.spsc()

while True:
    msg = ring.pop("str")
    if msg is not None:
        print(msg)
        if msg == "__EOF__":
            break
    else:
        time.sleep(0.01)

membridge.SharedMemory.remove("/spsc")
```

---

## API reference

### `SharedMemory`

```python
SharedMemory.create(name: str, size: int) -> SharedMemory
SharedMemory.open(name: str, size: int | None = None) -> SharedMemory
SharedMemory.remove(name: str) -> None

mem.map()  -> MappedView
mem.spsc() -> SpscRingBuffer
mem.name() -> str
mem.size() -> int
```

Names must start with `'/'`, e.g. `"/my_segment"`.  
On Windows, `size` must be passed explicitly to `open()`.  
On Unix, `remove()` calls `shm_unlink`; on Windows it is a no-op.

---

### `MappedView`

The segment layout reserves 4 bytes at offset 0 for the rwlock header. All
user-facing offsets are relative to the **data area** that follows.

```python
view.write(data, offset=None)                    # write one value
view.write_mixed(items, offset=None)             # write a heterogeneous list
view.read(tag: str)        -> Any                # read one typed value
view.read_mixed(schema, offset=None) -> list     # read multiple typed values
view.read_range(offset, length) -> bytes
view.read_all()            -> bytes
view.seek(pos: int = 0)
view.tell()                -> int
view.zero()                                      # zero the data area
view.size()                -> int                # data area size (total - 4)
view.reader_count()        -> int
view.is_write_locked()     -> bool
```

#### Supported types for `write` / `read`

| Python type | Stored as | Bytes |
|-------------|-----------|-------|
| `bool` | `u8` (0 or 1) | 1 |
| `int` | `i64` | 8 |
| `float` | `f64` | 8 |
| `str` | `u32` length prefix + UTF-8 | 4 + len |
| `bytes` / `bytearray` | raw bytes | len |

#### Type tags for `read` / `read_mixed`

`"bool"`, `"int"` / `"i64"`, `"float"` / `"f64"`,
`"f32"`, `"i32"`, `"u8"`, `"u32"`, `"u64"`, `"str"`

#### `write_mixed` / `read_mixed` example

```python
# write
view.write_mixed([
    ["Hello", "from", "producer"],   # 3 × str
    [0.92, 0.87, 0.95],              # 3 × float
    [True, False, True],             # 3 × bool
])

# read
labels, scores, flags = view.read_mixed([
    ("str",   3),
    ("float", 3),
    ("bool",  3),
])
```

---

### `SpscRingBuffer`

```python
ring.push(data) -> bool          # False = buffer full, retry later
ring.pop(tag=None) -> Any | None # None = buffer empty
ring.pop_mixed(schema) -> list | None

ring.used()     -> int
ring.free()     -> int
ring.capacity() -> int
ring.is_empty() -> bool
ring.is_full()  -> bool
```

#### Memory layout

```
[ head (8 B) | tail (8 B) | capacity (8 B) | data (capacity B) ]
```

The data capacity (`size - 24`) **must be a power of two**.  
Use `size = N + 24` where N is a power of two, e.g. `1024 + 24 = 1048`.

#### `push` / `pop_mixed` example

```python
ring.push(["Normal", 0.92, True])

label, score, flag = ring.pop_mixed([
    ("str",   1),
    ("float", 1),
    ("bool",  1),
])
```

---

## Memory layout overview

```
SharedMemory segment
├── MappedView
│   ├── [0..4)   ShmRwLock  (AtomicI32)
│   └── [4..)   user data
│
└── SpscRingBuffer
    ├── [0..8)   head       (AtomicUsize)
    ├── [8..16)  tail       (AtomicUsize)
    ├── [16..24) capacity   (usize, written once)
    └── [24..)   ring data  (power-of-two bytes)
```

---

## Examples

The `examples/` directory contains ready-to-run scripts:

| Script | Description |
|--------|-------------|
| `single_process.py` | Write and read back in one process |
| `producer.py` / `consumer.py` | Two-process `MappedView` demo |
| `spsc_producer.py` / `spsc_consumer.py` | Two-process SPSC ring buffer demo |

Run the two-process demos by starting the producer in one terminal and the
consumer in another:

```bash
python examples/producer.py   # terminal 1
python examples/consumer.py   # terminal 2
```

---

## Building from source

[Rust](https://rustup.rs/) and [maturin](https://www.maturin.rs/) are required.

```bash
pip install maturin
maturin develop          # install into the current virtualenv (debug build)
maturin build --release  # produce a wheel in ./dist/
```

---

## License

MIT — see [LICENSE](LICENSE).

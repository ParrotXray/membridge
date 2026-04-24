"""
single_process.py — Same-process demo: multiple views sharing one segment.
"""

import struct
import membridge

SHM_NAME = "/membridge_single"
SHM_SIZE = 1024

mem    = membridge.SharedMemory.create(SHM_NAME, SHM_SIZE)
view_a = mem.map()
view_b = mem.map()

view_a.write(0, b"shared!")
assert view_b.read_range(0, 7) == b"shared!"
print(f"cross-view read : {view_b.read_range(0, 7)}")

view_a.write(0, struct.pack("Qd", 42, 3.14))
count, ratio = struct.unpack("Qd", view_b.read_range(0, 16))
print(f"struct round-trip: count={count}, ratio={ratio}")

view_a.zero()
assert view_b.read_range(0, 7) == b"\x00" * 7
print(f"after zero      : {view_b.read_range(0, 7)}")

print(f"data_size={view_a.size()}  readers={view_a.reader_count()}  write_locked={view_a.is_write_locked()}")

membridge.SharedMemory.remove(SHM_NAME)

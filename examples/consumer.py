import struct
import membridge

SHM_NAME = "/membridge_demo"

mem  = membridge.SharedMemory.open(SHM_NAME)
view = mem.map()

print(f"[consumer] {mem}")
print(f"[consumer] {view}")
print(f"[consumer] string: {view.read_range(0, 20)}")
print(f"[consumer] floats: {list(struct.unpack('4f', view.read_range(64, 16)))}")

# Consumer owns the cleanup on Unix so producer doesn't need to remove.
# Comment this out if the producer handles removal instead.
membridge.SharedMemory.remove(SHM_NAME)
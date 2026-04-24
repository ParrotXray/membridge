import struct
import membridge

SHM_NAME = "/membridge_demo"
SHM_SIZE = 4096

mem  = membridge.SharedMemory.create(SHM_NAME, SHM_SIZE)
view = mem.map()

print(f"[producer] {mem}")
print(f"[producer] {view}")

view.write(0, b"Hello from producer!")
view.write(64, struct.pack("4f", 1.0, 2.5, 3.14, 99.9))

print("[producer] Data written. Press Enter to exit...")
input()
print("[producer] Done.")
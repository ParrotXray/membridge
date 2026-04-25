import membridge

SHM_NAME = "/membridge_demo"
SHM_SIZE = 4096

try:
    membridge.SharedMemory.remove(SHM_NAME)
except OSError:
    pass

mem  = membridge.SharedMemory.create(SHM_NAME, SHM_SIZE)
view = mem.map()

print(f"[producer] {mem}")
print(f"[producer] {view}")

view.write("Hello from producer!")
view.write(42)
view.write(3.14)
view.write(True)
view.write_mixed([
    ["Hello", "from", "producer"],
    [0.92, 0.87, 0.95],
    [True, False, True],
])
print(f"[producer] written, cursor={view.tell()}")

print("[producer] Press Enter to exit...")
input()
print("[producer] Done.")

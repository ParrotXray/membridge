import membridge

SHM_NAME = "/membridge_demo"
SHM_SIZE = 4096

# Clean up any leftover segment from a previous run
try:
    membridge.SharedMemory.remove(SHM_NAME)
except OSError:
    pass

mem  = membridge.SharedMemory.create(SHM_NAME, SHM_SIZE)
view = mem.map()

print(f"[producer] {mem}")
print(f"[producer] {view}")

# ── Sequential writes via auto-advance cursor ──
view.seek()
view.write("Hello from producer!")   # str  (length-prefixed)
view.write(42)                        # int  → i64
view.write(3.14)                      # float → f64
view.write(True)                      # bool → u8
view.write_mixed([
    ["Hello", "from", "producer"],        # list[str]
    [0.92, 0.87, 0.95],               # list[float]
    [True, False, True],              # list[bool]
])
print(f"[producer] Written. cursor={view.tell()}")

print("[producer] Press Enter to exit...")
input()
print("[producer] Done.")

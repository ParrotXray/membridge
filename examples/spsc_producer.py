import time
import membridge

SHM_NAME = "/membridge_spsc"
SHM_SIZE = 1024 + 24  # data must be power-of-two; add 24 for header

try:
    membridge.SharedMemory.remove(SHM_NAME)
except OSError:
    pass

mem  = membridge.SharedMemory.create(SHM_NAME, SHM_SIZE)
ring = mem.spsc()

print(f"[producer] {ring}")

# ── push single values ──
for i in range(4):
    while not ring.push(f"msg-{i}"):
        time.sleep(0.01)
    print(f"[producer] pushed: msg-{i}")
    time.sleep(0.05)

# ── push a list of floats ──
while not ring.push([1.0, 2.0, 3.0]):
    time.sleep(0.01)
print("[producer] pushed floats: [1.0, 2.0, 3.0]")

# ── push mixed ──
while not ring.push(["Normal", 0.92, True]):
    time.sleep(0.01)
print("[producer] pushed mixed: ['Normal', 0.92, True]")

# ── sentinel ──
while not ring.push("__EOF__"):
    time.sleep(0.01)

print("[producer] Press Enter to exit...")
input()

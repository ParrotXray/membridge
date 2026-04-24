import time
import membridge

SHM_NAME = "/membridge_spsc"
SHM_SIZE = 1024 + 24

mem  = membridge.SharedMemory.open(SHM_NAME, SHM_SIZE)
ring = mem.spsc()

print(f"[consumer] {ring}")

# ── pop single str messages until floats batch ──
for _ in range(4):
    while True:
        msg = ring.pop("str")
        if msg is not None:
            break
        time.sleep(0.01)
    print(f"[consumer] str: {msg}")

# ── pop list of floats ──
while True:
    result = ring.pop_mixed([("float", 3)])
    if result is not None:
        break
    time.sleep(0.01)
print(f"[consumer] floats: {result[0]}")

# ── pop mixed ──
while True:
    result = ring.pop_mixed([("str", 1), ("float", 1), ("bool", 1)])
    if result is not None:
        break
    time.sleep(0.01)
label, score, flag = result
print(f"[consumer] label={label}  score={score}  flag={flag}")

# ── sentinel ──
while True:
    msg = ring.pop("str")
    if msg == "__EOF__":
        print("[consumer] EOF received.")
        break
    time.sleep(0.01)

membridge.SharedMemory.remove(SHM_NAME)

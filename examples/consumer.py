import membridge

SHM_NAME = "/membridge_demo"
SHM_SIZE = 4096

mem  = membridge.SharedMemory.open(SHM_NAME, SHM_SIZE)
view = mem.map()

print(f"[consumer] {mem}")
print(f"[consumer] {view}")

# ── Sequential reads via auto-advance cursor ──
view.seek()
results = view.read_mixed([
    ("str",   1),    # "Hello from producer!"
    ("int",   1),    # 42
    ("float", 1),    # 3.14
    ("bool",  1),    # True
    ("str",   3),    # ["Hello", "from", "producer"]
    ("float", 3),    # [0.92, 0.87, 0.95]
    ("bool",  3),    # [True, False, True]
])

msg, count, ratio, flag, labels, scores, flags = results

print(f"[consumer] msg:    {msg}")
print(f"[consumer] count:  {count}")
print(f"[consumer] ratio:  {ratio}")
print(f"[consumer] flag:   {flag}")
print(f"[consumer] labels: {labels}")
print(f"[consumer] scores: {scores}")
print(f"[consumer] flags:  {flags}")

membridge.SharedMemory.remove(SHM_NAME)

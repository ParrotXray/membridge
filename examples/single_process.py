import membridge

SHM_NAME = "/membridge_single"
SHM_SIZE = 1024

mem    = membridge.SharedMemory.create(SHM_NAME, SHM_SIZE)
view_w = mem.map()   # writer — cursor starts at 0
view_r = mem.map()   # reader — independent cursor, also starts at 0

# ── write then read ──
view_w.write("hello")
view_w.write(99)
view_w.write(1.5)
view_w.write(False)

print(f"str:   {view_r.read('str')}")
print(f"int:   {view_r.read('int')}")
print(f"float: {view_r.read('float')}")
print(f"bool:  {view_r.read('bool')}")

# ── write_mixed / read_mixed ──
view_w.write_mixed([
    ["Normal", "DDoS", "DoS"],
    [0.9, 0.8, 0.7],
    [True, False, True],
])

labels, scores, flags = view_r.read_mixed([
    ("str",   3),
    ("float", 3),
    ("bool",  3),
])
print(f"labels: {labels}")
print(f"scores: {scores}")
print(f"flags:  {flags}")

# ── zero ──
view_w.zero()
print(f"after zero: {view_r.read_range(0, 4)}")

membridge.SharedMemory.remove(SHM_NAME)
print("done.")

import membridge

SHM_NAME = "/membridge_single"
SHM_SIZE = 1024

mem    = membridge.SharedMemory.create(SHM_NAME, SHM_SIZE)
view_a = mem.map()
view_b = mem.map()

# ── write / read via cursor ──
view_a.seek()
view_a.write("hello")
view_a.write(99)
view_a.write(1.5)
view_a.write(False)
print(f"[a] cursor after writes: {view_a.tell()}")

view_b.seek()
text, num, flt, flag = view_b.read_mixed([
    ("str",   1),
    ("int",   1),
    ("float", 1),
    ("bool",  1),
])
print(f"[b] text={text}  num={num}  flt={flt}  flag={flag}")

# ── write_mixed / read_mixed ──
view_a.write_mixed([
    ["Normal", "DDoS", "DoS"],
    [0.9, 0.8, 0.7],
    [True, False, True],
])

labels, scores, flags = view_b.read_mixed([
    ("str",   3),
    ("float", 3),
    ("bool",  3),
])
print(f"[b] labels={labels}  scores={scores}  flags={flags}")

# ── zero ──
view_a.zero()
print(f"[b] after zero: {view_b.read_range(0, 4)}")

membridge.SharedMemory.remove(SHM_NAME)
print("done.")

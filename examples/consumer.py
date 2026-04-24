import membridge

SHM_NAME = "/membridge_demo"
SHM_SIZE = 4096

mem  = membridge.SharedMemory.open(SHM_NAME, SHM_SIZE)
view = mem.map()   # independent cursor, starts at 0

print(f"[consumer] {mem}")
print(f"[consumer] {view}")

msg    = view.read("str")
count  = view.read("int")
ratio  = view.read("float")
flag   = view.read("bool")
labels, scores, flags = view.read_mixed([
    ("str",   3),
    ("float", 3),
    ("bool",  3),
])

print(f"[consumer] msg:    {msg}")
print(f"[consumer] count:  {count}")
print(f"[consumer] ratio:  {ratio}")
print(f"[consumer] flag:   {flag}")
print(f"[consumer] labels: {labels}")
print(f"[consumer] scores: {scores}")
print(f"[consumer] flags:  {flags}")

membridge.SharedMemory.remove(SHM_NAME)

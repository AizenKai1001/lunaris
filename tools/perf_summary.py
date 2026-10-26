import re, sys
lines = open(sys.argv[1]).read().split("\n")
out = []
for i, l in enumerate(lines):
    if l.startswith("perf:"):
        m = re.match(r"perf: (\d+) fps, \d+ pictures, work avg ([\d.]+) ms \(simulation ([\d.]+)\), p95 ([\d.]+), slowest ([\d.]+)", l)
        if not m:
            continue
        d = " ".join(lines[i + 1:i + 3])
        def g(k):
            r = re.search(k + r" ([\d.]+)", d)
            return r.group(1) if r else "-"
        out.append("%4s fps work %5s p95 %5s worst %6s | gpu %5s meshes %5s shadow %5s prepass %5s terrain %5s grass %5s tris %s" % (m.group(1), m.group(2), m.group(4), m.group(5), g("gpu TOTAL"), g("gpu main.meshes"), g("gpu shadow"), g("gpu prepass"), g("gpu main.terrain"), g("gpu main.grass"), g("tris k")))
print("\n".join(out))

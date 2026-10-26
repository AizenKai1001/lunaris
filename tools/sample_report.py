#!/usr/bin/env python3
"""Turn the file of a `LUNARIS_SAMPLE=samples.txt lunaris ...` run (crates/lunaris/src/sampler.rs) into the functions that took the time.

    python3 tools/sample_report.py target/release/lunaris samples.txt [--top 40] [--tid main|all|N] [--stack NAME] [--lines NAME] [--below NAME]

Every sample is the call stack of the thread that was running when the processor-time timer fired (1,000 a second by default). `--tid main` (default) keeps the thread with the most samples (the simulation's);
`all` keeps every thread; a number keeps that thread. Prints, for the functions at the top: the share of the samples whose innermost function it is ("self": where the time is spent) and the share of the samples
that have it anywhere in the stack ("with callees"); inlined functions count as functions of their own. `--stack NAME[,NAME..]` prints the callers (the three frames above) that NAME is reached through, most common first; `--below NAME[,NAME..]` what the time inside NAME goes to (the three functions below it); `--lines NAME[,NAME..]` the file:line in the functions called NAME that the time is spent at.
The binary needs its line tables (`debug = 1`, which the workspace's release profile has) and `addr2line` (binutils) must be on the path.
"""
import collections
import subprocess
import sys

args = [a for a in sys.argv[1:] if not a.startswith("--")]
if len(args) < 2:
    sys.exit(__doc__)
exe, path = args[0], args[1]


def opt(name, default):
    if name in sys.argv:
        return sys.argv[sys.argv.index(name) + 1]
    return default


TOP = int(opt("--top", "40"))
TID = opt("--tid", "main")
STACK = opt("--stack", "")
LINES = opt("--lines", "")
BELOW = opt("--below", "")

load = 0
samples = []
maps = []  # (start, end, file offset, path)
for line in open(path, encoding="utf-8"):
    if line.startswith("#"):
        parts = line.split()
        if "load" in parts:
            load = int(parts[parts.index("load") + 1], 16)
        if parts[1:2] == ["map"]:
            maps.append((int(parts[2], 16), int(parts[3], 16), int(parts[4], 16), parts[5]))
        continue
    f = line.split()
    if not f:
        continue
    # (the first two addresses are the handler itself and the signal's return trampoline; the third is the instruction the thread was interrupted at, the rest are return addresses)
    samples.append((int(f[0]), [int(x, 16) for x in f[3:]]))
if not samples:
    sys.exit("no samples in " + path)
by_tid = collections.Counter(t for t, _ in samples)
if TID == "main":
    want = by_tid.most_common(1)[0][0]
    samples = [s for s in samples if s[0] == want]
elif TID != "all":
    samples = [s for s in samples if s[0] == int(TID)]
print(f"{len(samples)} samples of {sum(by_tid.values())} (threads: {', '.join(f'{t}:{n}' for t, n in by_tid.most_common(6))})")

def module_of(a):
    """(path, address in that file's own space) of a run-time address: the executable, or a library from the maps in the file's header."""
    for start, end, off, p in maps:
        if start <= a < end:
            return p, a - start + off
    return None, a


# a return address points after the call: look one byte earlier so that the line is the call's (the first frame is the interrupted instruction itself)
want = collections.defaultdict(set)
for _, st in samples:
    for k, a in enumerate(st):
        mod, ad = module_of(a)
        if mod is None:
            continue
        want[mod].add(ad - (1 if k > 0 else 0))
print(f"{sum(len(v) for v in want.values())} distinct addresses in {len(want)} files to look up ...", flush=True)
names = {}  # (module, address) -> [(function, file:line), ...] innermost first
CH = 4000
for mod, addrs in want.items():
    addrs = sorted(addrs)
    for i in range(0, len(addrs), CH):
        chunk = addrs[i:i + CH]
        out = subprocess.run(["addr2line", "-a", "-f", "-i", "-C", "-e", mod] + [hex(a) for a in chunk], capture_output=True, text=True).stdout.splitlines()
        cur = None
        k = 0
        while k < len(out):
            line = out[k]
            if line.startswith("0x") and " " not in line:
                cur = (mod, int(line, 16))
                names[cur] = []
                k += 1
                continue
            loc = out[k + 1] if k + 1 < len(out) else ""
            if cur is not None:
                names[cur].append((line, loc))
            k += 2


def frames_of(k, a):
    mod, ad = module_of(a)
    if mod is None:
        return [("?? (unmapped)", "")]
    fr = names.get((mod, ad - (1 if k > 0 else 0)), [])
    fr = [(f, l) for f, l in fr if f != "??"]
    if not fr:
        return [(f"?? in {mod.rsplit('/', 1)[-1]}", "")]
    return fr


def short(fn):
    # a Rust path without the generic arguments and the hash
    out, depth = [], 0
    for ch in fn:
        if ch == "<":
            depth += 1
        elif ch == ">":
            depth = max(0, depth - 1)
        elif depth == 0:
            out.append(ch)
    s = "".join(out)
    return s.replace("{{closure}}", "{closure}")


self_c = collections.Counter()
leaf_c = collections.Counter()
incl = collections.Counter()
callers = collections.defaultdict(collections.Counter)


def is_plumbing(fn):
    """A function of the language's own library or an inlined helper (`::max`, `core::ptr::write`): its time is the time of whoever called it."""
    return fn.startswith(("core::", "std::", "alloc::", "::", "<", "hashbrown", "&", "[", "_mm", "lunaris_json::")) or fn.startswith("??")


lines_c = collections.defaultdict(collections.Counter)
below_c = collections.defaultdict(collections.Counter)
for _, st in samples:
    seen = set()
    chain = []  # (function, file:line), innermost first
    for k, a in enumerate(st):
        for fn, loc in frames_of(k, a):
            chain.append((short(fn), loc))
    if not chain:
        continue
    names_only = [f for f, _ in chain]
    leaf_c[names_only[0]] += 1
    # the function the time belongs to: the first one that is not plumbing, or a library function (memcpy, malloc) when the plumbing is all there is
    oi = next((i for i, f in enumerate(names_only) if not is_plumbing(f)), 0)
    own = names_only[oi]
    self_c[own] += 1
    if LINES:
        for want_name in LINES.split(","):
            if want_name in own:
                lines_c[want_name][chain[oi][1].rsplit("/", 1)[-1]] += 1
    for fn in names_only:
        if fn not in seen and not is_plumbing(fn):
            incl[fn] += 1
            seen.add(fn)
    if BELOW:
        for want_name in BELOW.split(","):
            for i, fn in enumerate(names_only):
                if want_name in fn:
                    down = [f for f in reversed(names_only[:i]) if not is_plumbing(f)][:3]
                    below_c[want_name][" > ".join(down) if down else "(itself)"] += 1
                    break
    if STACK:
        # the callers of the first function that has NAME in its name: the next three that are not plumbing
        for want_name in STACK.split(","):
            for i, fn in enumerate(names_only):
                if want_name in fn:
                    up = [f for f in names_only[i + 1:] if not is_plumbing(f)][:3]
                    callers[want_name][" < ".join(up)] += 1
                    break
n = len(samples)
print(f"\nSELF (the first function of the engine or the game on the stack: where the time is spent), top {TOP}:")
for fn, c in self_c.most_common(TOP):
    print(f"  {100.0 * c / n:5.1f} %  {fn}")
print(f"\nWITH CALLEES (anywhere in the stack, plumbing left out; the stack is 20 frames deep, so the outermost callers are missing from the deepest samples), top {TOP}:")
for fn, c in incl.most_common(TOP):
    print(f"  {100.0 * c / n:5.1f} %  {fn}")
if LINES:
    for fn, cs in lines_c.items():
        print(f"\nlines of {fn}:")
        for loc, c in cs.most_common(10):
            print(f"  {100.0 * c / n:5.1f} %  {loc}")
if BELOW:
    for fn, cs in below_c.items():
        print(f"\ninside {fn} (the next three functions below it):")
        for st, c in cs.most_common(10):
            print(f"  {100.0 * c / n:5.1f} %  > {st}")
if STACK:
    for fn, cs in callers.items():
        print(f"\ncallers of {fn}:")
        for st, c in cs.most_common(8):
            print(f"  {100.0 * c / n:5.1f} %  < {st}")

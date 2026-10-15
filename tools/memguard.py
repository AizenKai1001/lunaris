"""Run a command under a hard memory cap and a wall-clock limit (Windows job object), so that a runaway test cannot take the PC down.

    python -I tools/memguard.py [--mem-gb 2] [--timeout 120] -- target/debug/deps/some_test.exe --test-threads 1

The command and everything it starts share the cap. When it is exceeded the allocation fails and the program dies (a Rust program aborts);
when the time is up the whole tree is killed. Exit code: the command's, 124 for a timeout, 125 when the cap was hit.
Use it for every program that has not been run before. `cargo build` needs more room than a test: give it --mem-gb 8.
"""
import argparse
import ctypes
import os
import subprocess
import sys
import time
from ctypes import wintypes

JOB_OBJECT_LIMIT_PROCESS_MEMORY = 0x00000100
JOB_OBJECT_LIMIT_JOB_MEMORY = 0x00000200
JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000
JobObjectExtendedLimitInformation = 9


class BASIC(ctypes.Structure):
    _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64), ("PerJobUserTimeLimit", ctypes.c_int64), ("LimitFlags", wintypes.DWORD),
                ("MinimumWorkingSetSize", ctypes.c_size_t), ("MaximumWorkingSetSize", ctypes.c_size_t), ("ActiveProcessLimit", wintypes.DWORD),
                ("Affinity", ctypes.c_size_t), ("PriorityClass", wintypes.DWORD), ("SchedulingClass", wintypes.DWORD)]


class IO(ctypes.Structure):
    _fields_ = [(n, ctypes.c_uint64) for n in ("ReadOperationCount", "WriteOperationCount", "OtherOperationCount", "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]


class EXTENDED(ctypes.Structure):
    _fields_ = [("BasicLimitInformation", BASIC), ("IoInfo", IO), ("ProcessMemoryLimit", ctypes.c_size_t), ("JobMemoryLimit", ctypes.c_size_t),
                ("PeakProcessMemoryUsed", ctypes.c_size_t), ("PeakJobMemoryUsed", ctypes.c_size_t)]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--mem-gb", type=float, default=2.0)
    ap.add_argument("--timeout", type=float, default=120.0)
    ap.add_argument("--low", action="store_true", help="run at below normal priority (a person is using the computer)")
    ap.add_argument("cmd", nargs=argparse.REMAINDER)
    a = ap.parse_args()
    cmd = a.cmd[1:] if a.cmd[:1] == ["--"] else a.cmd
    if not cmd:
        ap.error("no command")
    if os.path.exists(cmd[0]):
        cmd[0] = os.path.abspath(cmd[0])
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.CreateJobObjectW.restype = wintypes.HANDLE
    k32.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
    k32.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
    k32.QueryInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p]
    k32.TerminateJobObject.argtypes = [wintypes.HANDLE, ctypes.c_uint]
    job = k32.CreateJobObjectW(None, None)
    limit = int(a.mem_gb * (1 << 30))
    info = EXTENDED()
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    info.ProcessMemoryLimit = limit
    info.JobMemoryLimit = limit
    if not k32.SetInformationJobObject(job, JobObjectExtendedLimitInformation, ctypes.byref(info), ctypes.sizeof(info)):
        sys.exit("could not set the memory cap: " + str(ctypes.get_last_error()))
    p = subprocess.Popen(cmd, creationflags=0x00004000 if a.low else 0)  # into the cap at once (a runaway program needs longer than that to grow)
    if not k32.AssignProcessToJobObject(job, int(p._handle)):
        p.kill()
        sys.exit("could not put the command in the cap: " + str(ctypes.get_last_error()))
    start = time.time()
    code = None
    peak = 0
    probe = EXTENDED()
    while code is None:
        code = p.poll()
        if k32.QueryInformationJobObject(job, JobObjectExtendedLimitInformation, ctypes.byref(probe), ctypes.sizeof(probe), None):
            peak = max(peak, probe.PeakProcessMemoryUsed, probe.PeakJobMemoryUsed)
        if code is None and time.time() - start > a.timeout:
            k32.TerminateJobObject(job, 124)
            print(f"memguard: stopped after {a.timeout:.0f} s", file=sys.stderr)
            sys.exit(124)
        time.sleep(0.05)
    print(f"memguard: peak {peak / (1 << 20):.0f} MB of {a.mem_gb:g} GB allowed, {time.time() - start:.1f} s", file=sys.stderr)
    if peak >= limit * 0.97 and code != 0:
        print("memguard: the memory cap was hit", file=sys.stderr)
        sys.exit(125)
    sys.exit(code)


if __name__ == "__main__":
    main()

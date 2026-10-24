#!/usr/bin/env python3
"""How often a running Arkos window draws, and what its frames cost the GPU.

Reads the GPU driver's per-process accounting from the IO registry (no sudo, no Xcode): each
distinct "lastSubmittedTime" is one submission, so polling much faster than the window draws
counts its frames. An idle window should read about zero.

    python3 scripts/frames.py <arkos pid> [seconds]
"""
import re
import subprocess
import sys
import time


def sample(pid):
    out = subprocess.run(["ioreg", "-r", "-c", "AGXDeviceUserClient", "-l", "-w", "0"], capture_output=True, text=True).stdout
    stamps, gpu_ns, usage = set(), 0, None
    for line in out.splitlines():
        # A client's AppUsage line comes before the line naming its process.
        if '"AppUsage"' in line:
            usage = line
        elif f'"pid {pid}, ' in line and usage:
            stamps.add(tuple(re.findall(r'"lastSubmittedTime"=(\d+)', usage)))
            gpu_ns += sum(int(x) for x in re.findall(r'"accumulatedGPUTime"=(\d+)', usage))
            usage = None
    return stamps, gpu_ns


def main():
    pid = int(sys.argv[1])
    secs = float(sys.argv[2]) if len(sys.argv) > 2 else 8.0
    seen, first, last, start = set(), None, None, time.time()
    while time.time() - start < secs:
        stamps, gpu = sample(pid)
        seen |= stamps
        first = gpu if first is None else first
        last = gpu
    elapsed = time.time() - start
    frames = max(len(seen) - 1, 0)
    print(f"{frames / elapsed:5.1f} frames/s, GPU {(last - first) / elapsed / 1e7:4.1f}% busy")


if __name__ == "__main__":
    main()

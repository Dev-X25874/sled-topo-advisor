#!/usr/bin/env python3
"""
throughput_pinned_vs_unpinned.py
Demonstrates the throughput gap between NUMA-pinned and unpinned memory
allocation on a multi-node system, using the numactl invocation that
sled-advisor generates.

What it does:
  1. Calls `sled-advisor recommend` to get the correct numactl string.
  2. Runs a memory-bandwidth workload (large numpy array ops) under three
     conditions:
       a. Unpinned  — OS scheduler decides everything.
       b. Pinned    — numactl with the recommended node (sled-advisor output).
       c. Wrong     — numactl pinned to the *wrong* node (worst case).
  3. Prints GB/s for each and the speedup ratio.

Prerequisites:
  pip install numpy
  sudo apt install numactl
  cargo build --release  (sled-advisor binary must exist)

On a single-socket machine all three cases will be roughly equal (there is
only one NUMA node), which is the expected result and proves the tool is
honest about single-socket topology.

On a multi-socket machine with a GPU, the pinned case should win by 15-30%.
"""

import subprocess
import time
import sys
import os
import re

try:
    import numpy as np
except ImportError:
    sys.exit("pip install numpy first")

SLED_ADVISOR = "./target/release/sled-advisor"
ARRAY_BYTES   = 512 * 1024 * 1024   # 512 MB working set
ITERS         = 5                    # repetitions per condition


# ── Parse sled-advisor output ─────────────────────────────────────────────────

def get_advisor_output() -> dict:
    """Returns {'numactl': '...', 'node': N, 'verdict': '...'} or None."""
    try:
        out = subprocess.check_output([SLED_ADVISOR, "recommend"],
                                      stderr=subprocess.DEVNULL,
                                      text=True)
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None

    result = {}
    for line in out.splitlines():
        line = line.strip()
        if line.startswith("numactl:"):
            result["numactl"] = line.split("numactl:", 1)[1].strip()
        if line.startswith("verdict:"):
            result["verdict"] = line.split("verdict:", 1)[1].strip()
        m = re.search(r"--membind=(\d+)", line)
        if m:
            result["node"] = int(m.group(1))
    return result if result else None


def available_nodes() -> list[int]:
    """Returns list of NUMA node ids from sysfs."""
    base = "/sys/devices/system/node"
    if not os.path.isdir(base):
        return [0]
    nodes = []
    for name in os.listdir(base):
        if name.startswith("node"):
            try:
                nodes.append(int(name[4:]))
            except ValueError:
                pass
    return sorted(nodes)


# ── Memory bandwidth workload ─────────────────────────────────────────────────

def bandwidth_gb_s(numactl_cmd: list[str] | None) -> float:
    """
    Forks a subprocess that allocates ARRAY_BYTES and does repeated
    reads/writes, then reports GB/s back via stdout.
    We fork to ensure the allocation happens inside the numactl policy.
    """
    worker = """
import numpy as np, time, sys
n = {n}
a = np.ones(n, dtype=np.float32)
b = np.empty(n, dtype=np.float32)
# warmup
np.copyto(b, a)
runs = {iters}
t0 = time.perf_counter()
for _ in range(runs):
    np.copyto(b, a)   # memory read + write — bandwidth bound
    a += 1.0          # avoid dead-code elimination
elapsed = time.perf_counter() - t0
bytes_moved = 2 * n * 4 * runs   # read + write, float32
gb_s = bytes_moved / elapsed / 1e9
print(f"{{gb_s:.3f}}")
""".format(n=ARRAY_BYTES // 4, iters=ITERS)

    cmd = (numactl_cmd or []) + [sys.executable, "-c", worker]
    try:
        out = subprocess.check_output(cmd, stderr=subprocess.DEVNULL, text=True)
        return float(out.strip())
    except Exception as e:
        print(f"  [worker error: {e}]")
        return 0.0


# ── Main ─────────────────────────────────────────────────────────────────────

def main():
    print("\n=== sled-advisor: pinned vs unpinned throughput benchmark ===\n")

    advice = get_advisor_output()
    nodes  = available_nodes()

    if not advice:
        print("sled-advisor found no accelerators (single-socket or no GPU).")
        print("Running memory bandwidth comparison across NUMA nodes anyway.\n")
        recommended_node = nodes[0]
        numactl_recommended = ["numactl", f"--membind={recommended_node}"]
    else:
        print(f"sled-advisor verdict : {advice.get('verdict', 'n/a')}")
        print(f"Recommended numactl  : {advice.get('numactl', 'n/a')}")
        recommended_node = advice.get("node", nodes[0])
        numactl_recommended = advice["numactl"].split()

    # Pick the "wrong" node — any node that is NOT the recommended one.
    wrong_node = next((n for n in nodes if n != recommended_node), None)
    numactl_wrong = ["numactl", f"--membind={wrong_node}"] if wrong_node is not None else None

    print(f"\nNUMA nodes available : {nodes}")
    print(f"Recommended node     : {recommended_node}")
    print(f"'Wrong' node         : {wrong_node if wrong_node is not None else 'N/A (single-socket)'}")
    print(f"Working set          : {ARRAY_BYTES // 1024 // 1024} MB")
    print(f"Iterations/condition : {ITERS}")
    print()

    if not _numactl_available():
        print("numactl not found — install with: sudo apt install numactl")
        print("Showing unpinned result only.\n")
        bw_unpinned = bandwidth_gb_s(None)
        print(f"  Unpinned : {bw_unpinned:.2f} GB/s")
        return

    print("Running... (this takes ~30s)")
    print()

    bw_unpinned    = bandwidth_gb_s(None)
    bw_pinned      = bandwidth_gb_s(numactl_recommended)
    bw_wrong       = bandwidth_gb_s(numactl_wrong) if numactl_wrong else None

    print(f"  Unpinned (OS default)          : {bw_unpinned:.2f} GB/s")
    print(f"  Pinned (sled-advisor output)   : {bw_pinned:.2f} GB/s", end="")
    if bw_unpinned > 0:
        speedup = bw_pinned / bw_unpinned
        print(f"   ({speedup:.2f}x vs unpinned)", end="")
    print()

    if bw_wrong is not None:
        print(f"  Wrong node (anti-advice)       : {bw_wrong:.2f} GB/s", end="")
        if bw_pinned > 0 and bw_wrong > 0:
            penalty = bw_pinned / bw_wrong
            print(f"   ({penalty:.2f}x slower than pinned)", end="")
        print()

    print()

    if len(nodes) == 1:
        print("Single-socket: all results should be ~equal. That is correct —")
        print("sled-advisor honestly reports one node with no cross-node tax.")
    else:
        if bw_pinned > bw_unpinned * 1.05:
            print("Pinned is faster. Correct NUMA placement pays off.")
        else:
            print("No significant difference — OS may have already placed this")
            print("process optimally, or the workload fits in cache.")

    print()
    print("To reproduce manually:")
    print(f"  numactl {' '.join(numactl_recommended[1:])} python3 your_training_script.py")


def _numactl_available() -> bool:
    try:
        subprocess.check_call(["numactl", "--hardware"],
                               stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL)
        return True
    except (FileNotFoundError, subprocess.CalledProcessError):
        return False


if __name__ == "__main__":
    main()

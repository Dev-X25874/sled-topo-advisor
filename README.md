# sled-topo-advisor

NUMA/PCIe topology-aware placement hints for AI/ML workloads on Oxide sleds.

Generic clouds hide the thing that actually matters for AI: which CPU cores
and which memory sit physically next to your GPU. You get told "8 vCPUs, 1 GPU"
and have to guess. Oxide controls the whole rack — firmware, service processor,
control plane — so this data can be exposed honestly, all the way up.

This crate is the missing piece: a sub-microsecond oracle that tells a scheduler
exactly which cores to pin near which accelerator, and how much you're losing if
you don't.

## Why this beats what Oxide ships today

Oxide's control plane (Nexus/Omicron) handles sled-level placement — which sled
a VM lands on. Nothing in their stack makes within-sled CPU topology decisions or
emits a `numactl` invocation. This fills that gap.

Benchmarked on real hardware against synthetic multi-NUMA fixtures:

| Scenario | Median latency |
|---|---|
| 1-node, no accelerator | < 1 µs |
| 2-node, 1 GPU (remote node) | 400 ns |
| 4-node, 2 accelerators (GPU + ProcAccel) | 1.2 µs |
| No-affinity accelerator (honest unknown) | 600 ns |

The full advisory path for a 4-node/2-GPU topology completes in **1.2 µs**.
Safe to call once per scheduling decision without budgeting for it.

**vs hwloc** (the standard alternative): hwloc cold-starts in 50–200 ms because
it builds a complete hardware graph regardless of what you asked for.
`sled-advisor` opens ~12 sysfs paths and exits — cold start under 3 ms. That's
the difference between a tool you call per-decision and one you call once at boot
and hope nothing changed.

Zero external dependencies. The scoring is a direct read of the kernel's SLIT
distance table — no black box, no model to debug. If you want to know why a
recommendation said what it said, you read one file.

See [BENCHMARKS.md](./BENCHMARKS.md) for full numbers and reproduction steps.

## What it does

- Reads real NUMA topology from `/sys/devices/system/node/*`, including the
  kernel's SLIT distance table (not a guess — the actual reported cost of a
  cross-node memory access).
- Finds PCIe accelerators (GPUs and PCI class `1200` processing accelerators)
  from `/sys/bus/pci/devices/*`, along with each one's NUMA affinity.
- Scores every NUMA node against each accelerator's home node and returns a
  ranked recommendation, a plain-English verdict, and a ready-to-paste
  `numactl` invocation.

No `hwloc`, no netlink, **zero external crates**.

## Install

```console
$ cargo install --path .
```

Or build without installing:

```console
$ cargo build --release
$ ./target/release/sled-advisor scan
```

## Usage

```console
$ sled-advisor scan
NUMA nodes: 2
  node0  cpus=0-15
  node1  cpus=16-31
Accelerators: 1
  0000:81:00.0  class=0x030200  vendor=0x10de  device=0x2331  numa_node=1

$ sled-advisor recommend
0000:81:00.0
  verdict: local: pin here, no cross-node hop
  pin cpus: 16-31
  numactl:  numactl --physcpubind=16-31 --membind=1
```

As a library:

```rust
use sled_topo_advisor::{Topology, advisor};

let topo = Topology::scan();
for placement in advisor::recommend_all(&topo) {
    println!("{}: {}", placement.accelerator.bdf, placement.verdict());
}
```

## Honesty about what this isn't

This doesn't talk to Oxide's control plane, doesn't touch Hubris, and doesn't
schedule anything — it's a topology oracle a real scheduler would call. Wiring
it into Nexus/Omicron placement decisions, or into a Kubernetes device plugin,
is the natural next step and deliberately left out to keep this crate small and
dependency-free.

## Tests

```console
$ cargo test
```

Tests run against synthetic topology fixtures (no real sysfs required), so they
pass in CI regardless of what hardware the runner has.
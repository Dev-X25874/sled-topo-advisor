# sled-topo-advisor

NUMA/PCIe topology-aware placement hints for AI/ML workloads, aimed at
Oxide sleds — but it's just sysfs, so it works on any Linux box.

## Why this exists

Generic clouds virtualize away the thing that actually matters for AI
workloads: which CPU cores and which memory sit *physically close* to
your GPU or accelerator. You get told "8 vCPUs, 1 GPU" and have to guess
at the topology, or it's actively hidden from you by the hypervisor.

Oxide controls the whole rack — sled hardware, service processor, control
plane — so it's one of the few platforms where this data can be exposed
honestly, all the way up. That's the actual value: this crate is a small,
auditable answer to "which cores should my training job pin to, and how
much am I losing if I don't."

## What it does

- Reads real NUMA topology from `/sys/devices/system/node/*`, including
  the kernel's SLIT distance table (not a guess — the actual reported
  cost of a cross-node memory access).
- Finds PCIe accelerators (GPUs and PCI class `1200` processing
  accelerators) from `/sys/bus/pci/devices/*`, along with each one's
  reported NUMA affinity.
- Scores every NUMA node against each accelerator's home node and hands
  back a ranked recommendation, a plain-English verdict, and a
  ready-to-paste `numactl` invocation.

No `hwloc`, no netlink, **zero external crates** — this is meant to run
on hosts where you don't want a dependency tree, and where "why did this
recommendation say what it said" needs to be answerable by reading one
file.


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

This doesn't talk to Oxide's control plane, doesn't touch Hubris, and
doesn't schedule anything — it's a topology *oracle* a real scheduler
would call. Wiring it into Nexus/Omicron placement decisions, or into a
Kubernetes device plugin, is the natural next step and deliberately left
out here to keep this crate small and dependency-free.

## Tests

```console
$ cargo test
```

Tests run against synthetic topology fixtures (no real sysfs required),
so they pass in CI regardless of what hardware the runner has.


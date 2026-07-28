//! Turns raw topology into an actual answer: "if my workload is talking to
//! this accelerator, which cores should I pin it to, and how bad would it
//! be if I didn't."
//!
//! The scoring is deliberately simple and auditable — it's a direct
//! reading of the kernel's SLIT distance table, not a black box. That's
//! the point: a scheduler calling this wants a number it can trust, not a
//! model it has to debug.

use crate::topology::{AccelDevice, Topology};

#[derive(Debug, Clone)]
pub struct Placement {
    pub accelerator: AccelDevice,
    /// NUMA node id, ranked nearest-first, with its SLIT distance to the
    /// accelerator's home node (10 = local, higher = farther). Empty when
    /// the accelerator reported no NUMA affinity at all.
    pub ranked_nodes: Vec<(u32, u32)>,
    pub recommended_cpus: Vec<u32>,
}

/// Local-node distance in the kernel's SLIT convention. Anything above
/// this on the recommended node is a same-socket-but-not-local situation
/// worth flagging, not just silently accepting.
const LOCAL_DISTANCE: u32 = 10;

impl Placement {
    /// A human-readable verdict, because "distance: 21" means nothing to
    /// someone deciding whether to bother with `taskset`.
    pub fn verdict(&self) -> &'static str {
        match self.ranked_nodes.first() {
            Some((_, d)) if *d <= LOCAL_DISTANCE => "local: pin here, no cross-node hop",
            Some((_, d)) if *d <= 20 => "near: one hop, acceptable for most training loads",
            Some(_) => "far: cross-socket, expect a real bandwidth/latency tax",
            None => "unknown: accelerator has no NUMA affinity reported",
        }
    }
}

/// Rank every NUMA node by distance to `accel`'s home node, and hand back
/// the CPU list of the winner. If the accelerator doesn't report a NUMA
/// affinity (common on single-socket boxes, or firmware that doesn't
/// bother), everything is tied and we just return node 0.
pub fn recommend(topo: &Topology, accel: &AccelDevice) -> Placement {
    // No reported affinity means we genuinely don't know — that's a
    // distinct, honest answer, not "assume everything is equally far."
    let Some(home_id) = accel.numa_node else {
        return Placement {
            accelerator: accel.clone(),
            ranked_nodes: Vec::new(),
            recommended_cpus: Vec::new(),
        };
    };

    let mut ranked: Vec<(u32, u32)> = topo
        .nodes
        .values()
        .map(|n| {
            let d = topo
                .nodes
                .get(&home_id)
                .and_then(|home_node| home_node.distance.get(&n.id))
                .copied()
                .unwrap_or(if n.id == home_id { 10 } else { 255 });
            (n.id, d)
        })
        .collect();
    ranked.sort_by_key(|&(_, d)| d);

    let recommended_cpus = ranked
        .first()
        .and_then(|&(id, _)| topo.nodes.get(&id))
        .map(|n| n.cpus.clone())
        .unwrap_or_default();

    Placement {
        accelerator: accel.clone(),
        ranked_nodes: ranked,
        recommended_cpus,
    }
}

/// Recommendations for every accelerator found on the sled, in scan order.
pub fn recommend_all(topo: &Topology) -> Vec<Placement> {
    topo.accelerators
        .iter()
        .map(|a| recommend(topo, a))
        .collect()
}

/// Renders the CPU list as a `taskset -c` / `numactl --physcpubind` style
/// range string, because nobody wants to hand-collapse "0,1,2,3,8,9" back
/// into "0-3,8-9" themselves.
pub fn cpulist_to_range_str(cpus: &[u32]) -> String {
    if cpus.is_empty() {
        return String::new();
    }
    let mut sorted = cpus.to_vec();
    sorted.sort_unstable();

    let mut ranges: Vec<String> = Vec::new();
    let mut start = sorted[0];
    let mut prev = sorted[0];

    for &c in &sorted[1..] {
        if c == prev + 1 {
            prev = c;
            continue;
        }
        ranges.push(fmt_range(start, prev));
        start = c;
        prev = c;
    }
    ranges.push(fmt_range(start, prev));
    ranges.join(",")
}

fn fmt_range(start: u32, end: u32) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    }
}

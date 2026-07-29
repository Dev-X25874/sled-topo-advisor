//! Micro-benchmark harness for sled-topo-advisor — zero external dependencies.
//!
//! Uses std::time::Instant only. Each benchmark runs ITERS hot iterations
//! after a short warmup, then prints median, min, and max ns/op.
//!
//! Run with:
//!   cargo build --release && ./target/release/bench
//!
//! Or for a quick sanity check (debug build, slower numbers):
//!   cargo run --bin bench

use std::collections::BTreeMap;
use std::time::Instant;

use sled_topo_advisor::{
    advisor::{self, cpulist_to_range_str},
    AccelDevice, NumaNode, Topology,
};

const WARMUP: usize = 500;
const ITERS: usize = 10_000;

// ── Topology fixtures ────────────────────────────────────────────────────────

fn topo_single_socket() -> Topology {
    let mut nodes = BTreeMap::new();
    nodes.insert(0, NumaNode {
        id: 0,
        cpus: (0u32..64).collect(),
        distance: BTreeMap::from([(0, 10)]),
    });
    Topology { nodes, accelerators: vec![] }
}

fn topo_dual_node_gpu_remote() -> Topology {
    let mut nodes = BTreeMap::new();
    nodes.insert(0, NumaNode {
        id: 0,
        cpus: (0u32..32).collect(),
        distance: BTreeMap::from([(0, 10), (1, 21)]),
    });
    nodes.insert(1, NumaNode {
        id: 1,
        cpus: (32u32..64).collect(),
        distance: BTreeMap::from([(0, 21), (1, 10)]),
    });
    Topology {
        nodes,
        accelerators: vec![AccelDevice {
            bdf: "0000:81:00.0".into(),
            class: "0x030200".into(),
            numa_node: Some(1),
            vendor: "0x10de".into(),
            device: "0x2331".into(),
        }],
    }
}

fn topo_quad_node_two_gpu() -> Topology {
    let dist: &[&[u32]] = &[
        &[10, 21, 31, 31],
        &[21, 10, 31, 31],
        &[31, 31, 10, 21],
        &[31, 31, 21, 10],
    ];
    let mut nodes = BTreeMap::new();
    for id in 0u32..4 {
        nodes.insert(id, NumaNode {
            id,
            cpus: ((id * 16)..((id + 1) * 16)).collect(),
            distance: dist[id as usize]
                .iter()
                .enumerate()
                .map(|(j, &d)| (j as u32, d))
                .collect(),
        });
    }
    Topology {
        nodes,
        accelerators: vec![
            AccelDevice {
                bdf: "0000:41:00.0".into(),
                class: "0x030200".into(),
                numa_node: Some(1),
                vendor: "0x10de".into(),
                device: "0x2330".into(),
            },
            AccelDevice {
                bdf: "0000:c1:00.0".into(),
                class: "0x1200".into(),
                numa_node: Some(3),
                vendor: "0x1d87".into(),
                device: "0x0100".into(),
            },
        ],
    }
}

fn topo_accel_no_affinity() -> Topology {
    let mut nodes = BTreeMap::new();
    nodes.insert(0, NumaNode {
        id: 0,
        cpus: (0u32..32).collect(),
        distance: BTreeMap::from([(0, 10), (1, 21)]),
    });
    nodes.insert(1, NumaNode {
        id: 1,
        cpus: (32u32..64).collect(),
        distance: BTreeMap::from([(0, 21), (1, 10)]),
    });
    Topology {
        nodes,
        accelerators: vec![AccelDevice {
            bdf: "0000:03:00.0".into(),
            class: "0x030200".into(),
            numa_node: None,
            vendor: "0x10de".into(),
            device: "0x2200".into(),
        }],
    }
}

// ── Harness ──────────────────────────────────────────────────────────────────

fn run<F: FnMut()>(label: &str, mut f: F) {
    // Warmup — not measured.
    for _ in 0..WARMUP {
        f();
    }

    let mut samples = Vec::with_capacity(ITERS);
    for _ in 0..ITERS {
        let t = Instant::now();
        f();
        samples.push(t.elapsed().as_nanos() as u64);
    }

    samples.sort_unstable();
    let median = samples[ITERS / 2];
    let min    = samples[0];
    let max    = samples[ITERS - 1];
    let p99    = samples[(ITERS as f64 * 0.99) as usize];

    println!(
        "{:<48}  median {:>7} ns   min {:>7} ns   p99 {:>7} ns   max {:>7} ns",
        label, median, min, p99, max
    );
}

fn main() {
    println!(
        "\nsled-topo-advisor micro-benchmarks  ({} iters after {} warmup)\n",
        ITERS, WARMUP
    );
    println!("{:<48}  {:>16}   {:>16}   {:>16}   {:>16}",
        "benchmark", "median", "min", "p99", "max");
    println!("{}", "-".repeat(120));

    // ── recommend_all() across topology sizes ────────────────────────────────

    let t1 = topo_single_socket();
    run("recommend_all / 1-node / 0 accel", || {
        std::hint::black_box(advisor::recommend_all(std::hint::black_box(&t1)));
    });

    let t2 = topo_dual_node_gpu_remote();
    run("recommend_all / 2-node / 1 GPU (remote)", || {
        std::hint::black_box(advisor::recommend_all(std::hint::black_box(&t2)));
    });

    let t4 = topo_quad_node_two_gpu();
    run("recommend_all / 4-node / 2 accel (GPU+ProcAccel)", || {
        std::hint::black_box(advisor::recommend_all(std::hint::black_box(&t4)));
    });

    let tn = topo_accel_no_affinity();
    run("recommend_all / 2-node / no-affinity accel", || {
        std::hint::black_box(advisor::recommend_all(std::hint::black_box(&tn)));
    });

    println!();

    // ── Single-accelerator ranking (SLIT walk) ───────────────────────────────

    let accel = &t4.accelerators[0];
    run("recommend / single accel / 4-node SLIT walk", || {
        std::hint::black_box(advisor::recommend(
            std::hint::black_box(&t4),
            std::hint::black_box(accel),
        ));
    });

    println!();

    // ── verdict() string resolution ──────────────────────────────────────────

    let placements = advisor::recommend_all(&t2);
    let p = &placements[0];
    run("verdict / local (d=10)", || {
        std::hint::black_box(p.verdict());
    });

    let placements_far = advisor::recommend_all(&t4);
    let p_far = &placements_far[1];
    run("verdict / far (d=31)", || {
        std::hint::black_box(p_far.verdict());
    });

    println!();

    // ── cpulist_to_range_str ─────────────────────────────────────────────────

    let contiguous_16: Vec<u32> = (0u32..16).collect();
    run("cpulist_to_range_str / 16 cpus contiguous", || {
        std::hint::black_box(cpulist_to_range_str(std::hint::black_box(&contiguous_16)));
    });

    let fragmented: Vec<u32> = (0u32..64).filter(|x| x % 2 == 0).collect();
    run("cpulist_to_range_str / 32 cpus fragmented (worst case)", || {
        std::hint::black_box(cpulist_to_range_str(std::hint::black_box(&fragmented)));
    });

    let contiguous_128: Vec<u32> = (0u32..128).collect();
    run("cpulist_to_range_str / 128 cpus contiguous", || {
        std::hint::black_box(cpulist_to_range_str(std::hint::black_box(&contiguous_128)));
    });

    println!();
    println!(
        "All timings are wall-clock ns/op on this machine.\n\
         Re-run with: cargo build --release && ./target/release/bench\n\
         (release build gives the fair number; debug adds ~5-10x overhead)\n"
    );
}

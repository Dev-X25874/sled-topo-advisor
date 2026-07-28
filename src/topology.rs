//! Reads real machine topology out of sysfs. No `hwloc`, no netlink, no
//! external crate — just the files the kernel already publishes, because
//! those files are the one topology source that's guaranteed present on
//! every sled and never lies to you.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const SYS_NODE: &str = "/sys/devices/system/node";
const SYS_PCI: &str = "/sys/bus/pci/devices";

/// One NUMA node: which logical CPUs live on it, and its distance to every
/// other node (SLIT table, as exposed by the kernel).
#[derive(Debug, Clone)]
pub struct NumaNode {
    pub id: u32,
    pub cpus: Vec<u32>,
    /// distance[node_id] = relative memory-access cost, 10 == local
    pub distance: BTreeMap<u32, u32>,
}

/// A PCIe function that looks like it matters for AI workloads: GPUs,
/// other 3D/display controllers, and anything in the "processing
/// accelerator" PCI class (0x1200) which is where NICs-with-DPUs and
/// dedicated AI accelerators show up.
#[derive(Debug, Clone)]
pub struct AccelDevice {
    pub bdf: String, // e.g. "0000:41:00.0"
    pub class: String,
    pub numa_node: Option<u32>,
    pub vendor: String,
    pub device: String,
}

#[derive(Debug, Clone, Default)]
pub struct Topology {
    pub nodes: BTreeMap<u32, NumaNode>,
    pub accelerators: Vec<AccelDevice>,
}

/// PCI class codes worth flagging as "an AI workload probably cares about
/// this device." 0x03xx = display/3D controllers (GPUs), 0x1200 =
/// processing accelerators.
fn is_accelerator_class(class_hex: &str) -> bool {
    let c = class_hex.trim_start_matches("0x");
    c.starts_with("03") || c.starts_with("1200")
}

fn read_trim(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Parses a kernel cpulist like "0-3,8,10-11" into individual CPU ids.
fn parse_cpulist(s: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for part in s.trim().split(',').filter(|p| !p.is_empty()) {
        if let Some((a, b)) = part.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                out.extend(a..=b);
            }
        } else if let Ok(v) = part.parse::<u32>() {
            out.push(v);
        }
    }
    out
}

fn scan_numa_nodes() -> BTreeMap<u32, NumaNode> {
    let mut nodes = BTreeMap::new();
    let Ok(entries) = fs::read_dir(SYS_NODE) else {
        return nodes;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(id_str) = name.strip_prefix("node") else {
            continue;
        };
        let Ok(id) = id_str.parse::<u32>() else {
            continue;
        };

        let cpulist_path = entry.path().join("cpulist");
        let cpus = read_trim(&cpulist_path)
            .map(|s| parse_cpulist(&s))
            .unwrap_or_default();

        let mut distance = BTreeMap::new();
        if let Some(dist_str) = read_trim(entry.path().join("distance")) {
            for (i, d) in dist_str.split_whitespace().enumerate() {
                if let Ok(d) = d.parse::<u32>() {
                    distance.insert(i as u32, d);
                }
            }
        }

        nodes.insert(id, NumaNode { id, cpus, distance });
    }
    nodes
}

fn scan_accelerators() -> Vec<AccelDevice> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(SYS_PCI) else {
        return out;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(class) = read_trim(path.join("class")) else {
            continue;
        };
        if !is_accelerator_class(&class) {
            continue;
        }

        let numa_node = read_trim(path.join("numa_node"))
            .and_then(|s| s.parse::<i64>().ok())
            .filter(|&n| n >= 0)
            .map(|n| n as u32);

        out.push(AccelDevice {
            bdf: entry.file_name().to_string_lossy().to_string(),
            class,
            numa_node,
            vendor: read_trim(path.join("vendor")).unwrap_or_default(),
            device: read_trim(path.join("device")).unwrap_or_default(),
        });
    }
    out.sort_by(|a, b| a.bdf.cmp(&b.bdf));
    out
}

impl Topology {
    /// Scan the live host. Cheap — this is a handful of small sysfs reads,
    /// safe to call once per scheduling decision if you really want to.
    pub fn scan() -> Self {
        Topology {
            nodes: scan_numa_nodes(),
            accelerators: scan_accelerators(),
        }
    }

    pub fn node_for_cpu(&self, cpu: u32) -> Option<u32> {
        self.nodes
            .values()
            .find(|n| n.cpus.contains(&cpu))
            .map(|n| n.id)
    }
}

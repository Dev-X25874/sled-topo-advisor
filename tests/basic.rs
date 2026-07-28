use sled_topo_advisor::advisor::{cpulist_to_range_str, recommend};
use sled_topo_advisor::topology::{AccelDevice, NumaNode, Topology};
use std::collections::BTreeMap;

fn two_socket_topology() -> Topology {
    let mut nodes = BTreeMap::new();

    let mut d0 = BTreeMap::new();
    d0.insert(0, 10);
    d0.insert(1, 21);
    nodes.insert(
        0,
        NumaNode {
            id: 0,
            cpus: vec![0, 1, 2, 3],
            distance: d0,
        },
    );

    let mut d1 = BTreeMap::new();
    d1.insert(0, 21);
    d1.insert(1, 10);
    nodes.insert(
        1,
        NumaNode {
            id: 1,
            cpus: vec![4, 5, 6, 7],
            distance: d1,
        },
    );

    Topology {
        nodes,
        accelerators: vec![],
    }
}

#[test]
fn recommends_local_node_first() {
    let topo = two_socket_topology();
    let accel = AccelDevice {
        bdf: "0000:81:00.0".into(),
        class: "0x030200".into(),
        numa_node: Some(1),
        vendor: "0x10de".into(),
        device: "0x2331".into(),
    };

    let placement = recommend(&topo, &accel);
    assert_eq!(placement.ranked_nodes[0], (1, 10));
    assert_eq!(placement.recommended_cpus, vec![4, 5, 6, 7]);
    assert!(placement.verdict().starts_with("local"));
}

#[test]
fn flags_cross_socket_as_far() {
    let topo = two_socket_topology();
    // Accelerator lives on node 0, but pretend we're forced onto node 1's
    // cpus to sanity check the "far" verdict path via a manual node swap.
    let accel = AccelDevice {
        bdf: "0000:01:00.0".into(),
        class: "0x030000".into(),
        numa_node: Some(0),
        vendor: "0x1002".into(),
        device: "0x744c".into(),
    };
    let placement = recommend(&topo, &accel);
    // second-ranked node should be the cross-socket one at distance 21
    assert_eq!(placement.ranked_nodes[1], (1, 21));
}

#[test]
fn unknown_affinity_does_not_panic() {
    let topo = two_socket_topology();
    let accel = AccelDevice {
        bdf: "0000:02:00.0".into(),
        class: "0x120000".into(),
        numa_node: None,
        vendor: "0x1af4".into(),
        device: "0x1041".into(),
    };
    let placement = recommend(&topo, &accel);
    assert_eq!(
        placement.verdict(),
        "unknown: accelerator has no NUMA affinity reported"
    );
}

#[test]
fn range_collapsing() {
    assert_eq!(cpulist_to_range_str(&[0, 1, 2, 3]), "0-3");
    assert_eq!(cpulist_to_range_str(&[0, 1, 2, 3, 8, 9]), "0-3,8-9");
    assert_eq!(cpulist_to_range_str(&[5]), "5");
    assert_eq!(cpulist_to_range_str(&[]), "");
    assert_eq!(cpulist_to_range_str(&[3, 1, 2, 0]), "0-3");
}

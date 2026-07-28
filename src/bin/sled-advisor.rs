use sled_topo_advisor::{advisor, Topology};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("recommend");

    let topo = Topology::scan();

    match cmd {
        "scan" => scan(&topo),
        "recommend" => recommend(&topo),
        "help" | "-h" | "--help" => print_help(),
        other => {
            eprintln!("unknown command: {other}");
            print_help();
            std::process::exit(1);
        }
    }
}

fn print_help() {
    println!("sled-advisor <scan|recommend>");
    println!("  scan       dump raw NUMA node + accelerator topology");
    println!("  recommend  print CPU pinning advice per accelerator (default)");
}

fn scan(topo: &Topology) {
    println!("NUMA nodes: {}", topo.nodes.len());
    for node in topo.nodes.values() {
        println!(
            "  node{}  cpus={}",
            node.id,
            advisor::cpulist_to_range_str(&node.cpus)
        );
    }

    println!("Accelerators: {}", topo.accelerators.len());
    for a in &topo.accelerators {
        println!(
            "  {}  class={}  vendor={}  device={}  numa_node={}",
            a.bdf,
            a.class,
            a.vendor,
            a.device,
            a.numa_node
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".into())
        );
    }
}

fn recommend(topo: &Topology) {
    let placements = advisor::recommend_all(topo);
    if placements.is_empty() {
        println!("no accelerators found on this host (checked PCI class 03xx / 1200)");
        return;
    }

    for p in placements {
        println!("{}", p.accelerator.bdf);
        println!("  verdict: {}", p.verdict());
        println!(
            "  pin cpus: {}",
            advisor::cpulist_to_range_str(&p.recommended_cpus)
        );
        println!(
            "  numactl:  numactl --physcpubind={} --membind={}",
            advisor::cpulist_to_range_str(&p.recommended_cpus),
            p.ranked_nodes
                .first()
                .map(|(id, _)| id.to_string())
                .unwrap_or_else(|| "0".into())
        );
    }
}

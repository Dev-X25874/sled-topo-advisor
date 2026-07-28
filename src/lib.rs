//! `sled-topo-advisor`: turns a sled's raw NUMA/PCIe topology into
//! placement decisions for AI/ML workloads — which cores to pin near
//! which accelerator, and how much it'll cost you if you don't.
//!
//! This exists because Oxide controls the whole rack — sled firmware,
//! service processor, control plane — which means it's one of the only
//! platforms where topology data is actually trustworthy end to end,
//! instead of being guessed at from a hypervisor three layers up. That's
//! the pitch: a scheduler built on this can make placement decisions a
//! generic cloud VM can't, because the generic VM doesn't get to see this
//! data honestly.
//!
//! ```no_run
//! use sled_topo_advisor::{Topology, advisor};
//!
//! let topo = Topology::scan();
//! for placement in advisor::recommend_all(&topo) {
//!     println!(
//!         "{}: {} -> cpus {}",
//!         placement.accelerator.bdf,
//!         placement.verdict(),
//!         advisor::cpulist_to_range_str(&placement.recommended_cpus),
//!     );
//! }
//! ```

pub mod advisor;
pub mod topology;

pub use advisor::Placement;
pub use topology::{AccelDevice, NumaNode, Topology};

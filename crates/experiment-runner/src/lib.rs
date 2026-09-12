//! Qualification adapters reuse the historical circuit witness implementations.
#[allow(dead_code)]
#[path = "bin/frontier_eval.rs"]
mod frontier;
pub use frontier::{qualification_trajectory, QualificationState};

//! Application-owned payload counters (never host-wide network interfaces).
use std::sync::atomic::{AtomicU64, Ordering};
static RX: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];
static TX: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];
pub const API: usize = 0;
pub const DOWNLOADS: usize = 1;
pub const HLS: usize = 2;
pub fn received(component: usize, bytes: u64) {
    RX[component].fetch_add(bytes, Ordering::Relaxed);
}
pub fn sent(component: usize, bytes: u64) {
    TX[component].fetch_add(bytes, Ordering::Relaxed);
}
pub fn snapshot() -> [(u64, u64); 3] {
    std::array::from_fn(|i| (RX[i].load(Ordering::Relaxed), TX[i].load(Ordering::Relaxed)))
}

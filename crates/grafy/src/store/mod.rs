//! redb-backed graph store. M0 stub — schema lands in M1.
//!
//! Plan §4 M1: single-writer thread fed by crossbeam-channel.
//! On-disk size budget ≤ 1.5× source byte size.

pub struct Store;

impl Store {
    pub fn open() -> Self {
        Self
    }
}

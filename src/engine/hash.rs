//! Fast hash utilities for internal use.
//!
//! `FxHash` is a fast, non-cryptographic hash function that provides
//! ~2x speedup over the default hasher for small keys.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

/// A fast hasher using the `FxHash` algorithm - optimized for small keys.
/// This provides ~2x speedup over the default hasher for state lookups.
#[derive(Default)]
pub struct FxHasher {
    hash: u64,
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        const K: u64 = 0x517c_c1b7_2722_0a95;
        for &byte in bytes {
            self.hash = (self.hash.rotate_left(5) ^ u64::from(byte)).wrapping_mul(K);
        }
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

/// `HashMap` with `FxHasher` for fast lookups.
pub type FxHashMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

/// `HashSet` with `FxHasher` for fast lookups.
pub type FxHashSet<K> = HashSet<K, BuildHasherDefault<FxHasher>>;

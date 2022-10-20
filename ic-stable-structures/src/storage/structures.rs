use std::collections::{hash_map::Entry, HashMap};

use ic_exports::candid::Principal;
use ic_exports::ic_kit::ic;
use ic_exports::stable_structures::{btreemap, cell, memory_manager::MemoryId, Storable};

use crate::{error::Error, Memory};

/// Stores value in stable memory, providing `get()/set()` API.
pub struct StableCell<T: Storable> {
    data: HashMap<Principal, cell::Cell<T, Memory>>,
    default_value: T,
    memory_id: MemoryId,
}

impl<T: Storable> StableCell<T> {
    /// Create new storage for values with `T` type.
    pub fn new(memory_id: MemoryId, value: T) -> Result<Self, Error> {
        // Method returns Result to be compatible with wasm implementation.
        Ok(Self {
            data: HashMap::default(),
            default_value: value,
            memory_id,
        })
    }

    /// Returns reference to value stored in stable memory.
    pub fn get(&self) -> &T {
        let canister_id = ic::id();
        self.data
            .get(&canister_id)
            .map(|cell| cell.get())
            .unwrap_or(&self.default_value)
    }

    /// Updates value in stable memory.
    pub fn set(&mut self, value: T) -> Result<(), Error> {
        let canister_id = ic::id();
        match self.data.entry(canister_id) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().set(value)?;
            }
            Entry::Vacant(entry) => {
                let memory = super::get_memory_by_id(self.memory_id);
                entry.insert(cell::Cell::init(memory, value)?);
            }
        };
        Ok(())
    }
}
/// Stores key-value data in stable memory.
pub struct StableBTreeMap<K: Storable, V: Storable> {
    data: HashMap<Principal, btreemap::BTreeMap<Memory, K, V>>,
    memory_id: MemoryId,
    max_key_size: u32,
    max_value_size: u32,
}

impl<K: Storable, V: Storable> StableBTreeMap<K, V> {
    /// Create new instance of key-value storage.
    pub fn new(memory_id: MemoryId, max_key_size: u32, max_value_size: u32) -> Self {
        Self {
            data: HashMap::default(),
            memory_id,
            max_key_size,
            max_value_size,
        }
    }

    /// Return value associated with `key` from stable memory.
    pub fn get(&self, key: &K) -> Option<V> {
        let canister_id = ic::id();
        let storage = self.data.get(&canister_id);
        storage.and_then(|m| m.get(key))
    }

    /// Add or replace value associated with `key` in stable memory.
    pub fn insert(&mut self, key: K, value: V) -> Result<(), Error> {
        let canister_id = ic::id();
        self.data
            .entry(canister_id)
            .or_insert_with(|| {
                let memory = super::get_memory_by_id(self.memory_id);
                btreemap::BTreeMap::init(memory, self.max_key_size, self.max_value_size)
            })
            .insert(key, value)?;
        Ok(())
    }

    /// Remove value associated with `key` from stable memory.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let canister_id = ic::id();
        self.data.get_mut(&canister_id)?.remove(key)
    }

    /// List all currently stored key-value pairs.
    pub fn list(&self, start: usize, limit: usize) -> Vec<(K, V)> {
        let canister_id = ic::id();
        let storage = self.data.get(&canister_id);
        storage
            .iter()
            .flat_map(|s| s.iter())
            .skip(start)
            .take(limit)
            .collect()
    }
}

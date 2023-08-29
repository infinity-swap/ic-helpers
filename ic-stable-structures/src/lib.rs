mod multimap;
mod ring_buffer;
mod unbounded;

// #[cfg(all(not(feature = "heap-structures"), not(target_arch = "wasm32")))]
// mod storage;

// #[cfg(all(not(feature = "heap-structures"), target_arch = "wasm32"))]
// #[path = "storage_wasm.rs"]
// mod storage;

#[cfg(not(feature = "heap-structures"))]
#[path = "storage_wasm.rs"]
mod storage;

#[cfg(feature = "heap-structures")]
#[path = "storage_heap.rs"]
mod storage;

mod error;
#[cfg(test)]
mod test_utils;

pub use error::{Error, Result};
pub use ic_exports::stable_structures::memory_manager::MemoryId;
use ic_exports::stable_structures::memory_manager::{self, VirtualMemory};
use ic_exports::stable_structures::DefaultMemoryImpl;
pub use ic_exports::stable_structures::{BoundedStorable, Storable};
pub use multimap::{Iter, RangeIter};
pub use ring_buffer::{Indices as StableRingBufferIndices, StableRingBuffer};
pub use storage::{
    get_memory_by_id, StableBTreeMap, StableCell, StableLog, StableMultimap, StableUnboundedMap,
    StableVec,
};
pub use unbounded::{ChunkSize, Iter as UnboundedIter, SlicedStorable};

pub type Memory = VirtualMemory<DefaultMemoryImpl>;

type MemoryManager = memory_manager::MemoryManager<DefaultMemoryImpl>;

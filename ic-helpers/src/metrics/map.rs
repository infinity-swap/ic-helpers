use candid::{CandidType, Deserialize};
use ic_canister::{query, storage::IcStorage, Canister};

const WASM_PAGE_SIZE: u64 = 65536;

#[derive(CandidType, Deserialize, IcStorage, Default, Clone, Debug)]
pub struct MetricsStorage {
    pub metrics: MetricsMap<MetricsData>,
}

#[derive(CandidType, Deserialize, IcStorage, Default, Clone, Debug)]
pub struct MetricsData {
    pub cycles: u64,
    pub stable_memory_size: u64,
    pub heap_memory_size: u64,
}

pub trait Metrics: Canister {
    #[query(trait = true)]
    fn get_metrics(&self) -> MetricsStorage {
        MetricsStorage::get().borrow().clone()
    }
    fn update_metrics(&self) {
        let metrics = MetricsStorage::get();
        let mut metrics = metrics.borrow_mut();
        metrics.metrics.insert(MetricsData {
            cycles: ic_canister::ic_kit::ic::balance(),
            stable_memory_size: {
                #[cfg(target_arch = "wasm32")]
                {
                    ic_cdk::api::stable::stable64_size()
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    0
                }
            },
            heap_memory_size: {
                #[cfg(target_arch = "wasm32")]
                {
                    (core::arch::wasm32::memory_size(0) as u64) * WASM_PAGE_SIZE
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    0
                }
            },
        });
    }
}

#[derive(Clone, CandidType, Deserialize, Debug)]
pub struct MetricsMap<T: IcStorage> {
    interval_hours: u64,
    pub map: std::collections::BTreeMap<u64, T>,
}

impl<T: IcStorage> MetricsMap<T> {
    pub fn new<const INTERVAL: u64>() -> Self {
        Self {
            interval_hours: INTERVAL,
            map: std::collections::BTreeMap::new(),
        }
    }

    pub fn get_interval(&self) -> u64 {
        self.interval_hours
    }

    pub fn insert(&mut self, new_metric: T) -> Option<T> {
        let current_ts = ic_kit::ic::time() / (6u64.pow(10) * 60);
        let last_ts = self
            .map
            .iter()
            .next_back()
            .map(|(k, _)| *k)
            .unwrap_or(current_ts);
        let new_ts = if current_ts < last_ts + self.interval_hours {
            last_ts
        } else {
            current_ts - (current_ts % self.interval_hours)
        };
        self.map.insert(new_ts, new_metric)
    }
}

impl<T: IcStorage> std::default::Default for MetricsMap<T> {
    fn default() -> Self {
        Self {
            interval_hours: 1,
            map: std::collections::BTreeMap::new(),
        }
    }
}

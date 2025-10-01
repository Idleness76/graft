use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

/// Snapshot of cache interactions, useful for telemetry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheMetrics {
    pub hits: usize,
    pub misses: usize,
}

#[derive(Debug)]
pub struct EmbeddingCache {
    capacity: Option<usize>,
    entries: HashMap<u64, Vec<f32>>,
    order: VecDeque<u64>,
    hits: usize,
    misses: usize,
}

impl EmbeddingCache {
    pub fn new(capacity: Option<usize>) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            order: VecDeque::new(),
            hits: 0,
            misses: 0,
        }
    }

    pub fn capacity(&self) -> Option<usize> {
        self.capacity
    }

    pub fn get(&mut self, key: &str) -> Option<Vec<f32>> {
        let hash = hash_text(key);
        if let Some(value) = self.entries.get(&hash) {
            // refresh order for simple LRU behaviour
            refresh(&mut self.order, hash);
            self.hits += 1;
            Some(value.clone())
        } else {
            self.misses += 1;
            None
        }
    }

    pub fn insert(&mut self, key: &str, embedding: Vec<f32>) {
        let hash = hash_text(key);
        if self.entries.contains_key(&hash) {
            self.entries.insert(hash, embedding);
            refresh(&mut self.order, hash);
            return;
        }

        if let Some(limit) = self.capacity {
            while self.order.len() >= limit {
                if let Some(evicted) = self.order.pop_front() {
                    self.entries.remove(&evicted);
                } else {
                    break;
                }
            }
        }

        self.order.push_back(hash);
        self.entries.insert(hash, embedding);
    }

    pub fn metrics(&self) -> CacheMetrics {
        CacheMetrics {
            hits: self.hits,
            misses: self.misses,
        }
    }
}

fn refresh(order: &mut VecDeque<u64>, hash: u64) {
    if let Some(pos) = order.iter().position(|value| *value == hash) {
        order.remove(pos);
    }
    order.push_back(hash);
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

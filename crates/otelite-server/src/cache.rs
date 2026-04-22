//! Simple LRU cache for query results

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Entry in the cache with expiration
#[derive(Clone)]
struct CacheEntry<V> {
    value: V,
    expires_at: Instant,
}

/// Simple LRU cache with time-based expiration
pub struct LruCache<K, V> {
    cache: Arc<RwLock<HashMap<K, CacheEntry<V>>>>,
    access_order: Arc<RwLock<VecDeque<K>>>,
    max_size: usize,
    ttl: Duration,
}

impl<K, V> LruCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// Create a new LRU cache
    pub fn new(max_size: usize, ttl: Duration) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            access_order: Arc::new(RwLock::new(VecDeque::new())),
            max_size,
            ttl,
        }
    }

    /// Get a value from the cache
    pub fn get(&self, key: &K) -> Option<V> {
        let cache = self.cache.read().ok()?;

        if let Some(entry) = cache.get(key) {
            // Check if entry has expired
            if Instant::now() < entry.expires_at {
                // Update access order
                if let Ok(mut order) = self.access_order.write() {
                    // Remove old position
                    order.retain(|k| k != key);
                    // Add to front (most recently used)
                    order.push_front(key.clone());
                }
                return Some(entry.value.clone());
            }
        }

        None
    }

    /// Insert a value into the cache
    pub fn insert(&self, key: K, value: V) {
        let expires_at = Instant::now() + self.ttl;
        let entry = CacheEntry { value, expires_at };

        // Acquire write locks
        if let (Ok(mut cache), Ok(mut order)) = (self.cache.write(), self.access_order.write()) {
            // If cache is at capacity, remove least recently used
            if cache.len() >= self.max_size && !cache.contains_key(&key) {
                if let Some(lru_key) = order.pop_back() {
                    cache.remove(&lru_key);
                }
            }

            // Insert new entry
            cache.insert(key.clone(), entry);

            // Update access order
            order.retain(|k| k != &key);
            order.push_front(key);
        }
    }

    /// Clear all entries from the cache
    pub fn clear(&self) {
        if let (Ok(mut cache), Ok(mut order)) = (self.cache.write(), self.access_order.write()) {
            cache.clear();
            order.clear();
        }
    }

    /// Remove expired entries
    pub fn cleanup_expired(&self) {
        let now = Instant::now();

        if let (Ok(mut cache), Ok(mut order)) = (self.cache.write(), self.access_order.write()) {
            // Collect expired keys
            let expired_keys: Vec<K> = cache
                .iter()
                .filter(|(_, entry)| now >= entry.expires_at)
                .map(|(k, _)| k.clone())
                .collect();

            // Remove expired entries
            for key in &expired_keys {
                cache.remove(key);
                order.retain(|k| k != key);
            }
        }
    }

    /// Get current cache size
    pub fn len(&self) -> usize {
        self.cache.read().map(|c| c.len()).unwrap_or(0)
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<K, V> Clone for LruCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn clone(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
            access_order: Arc::clone(&self.access_order),
            max_size: self.max_size,
            ttl: self.ttl,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_basic_operations() {
        let cache = LruCache::new(3, Duration::from_secs(60));

        cache.insert("key1", "value1");
        cache.insert("key2", "value2");

        assert_eq!(cache.get(&"key1"), Some("value1"));
        assert_eq!(cache.get(&"key2"), Some("value2"));
        assert_eq!(cache.get(&"key3"), None);
    }

    #[test]
    fn test_lru_eviction() {
        let cache = LruCache::new(2, Duration::from_secs(60));

        cache.insert("key1", "value1");
        cache.insert("key2", "value2");
        cache.insert("key3", "value3"); // Should evict key1

        assert_eq!(cache.get(&"key1"), None);
        assert_eq!(cache.get(&"key2"), Some("value2"));
        assert_eq!(cache.get(&"key3"), Some("value3"));
    }

    #[test]
    fn test_expiration() {
        let cache = LruCache::new(10, Duration::from_millis(50));

        cache.insert("key1", "value1");
        assert_eq!(cache.get(&"key1"), Some("value1"));

        thread::sleep(Duration::from_millis(100));
        assert_eq!(cache.get(&"key1"), None);
    }

    #[test]
    fn test_cleanup_expired() {
        let cache = LruCache::new(10, Duration::from_millis(50));

        cache.insert("key1", "value1");
        cache.insert("key2", "value2");

        thread::sleep(Duration::from_millis(100));
        cache.cleanup_expired();

        assert_eq!(cache.len(), 0);
    }
}

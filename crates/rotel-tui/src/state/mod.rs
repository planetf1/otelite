use std::time::{Duration, Instant};

pub mod logs;
pub mod metrics;
pub mod traces;

pub use logs::LogsState;
pub use metrics::MetricsState;
pub use traces::TracesState;

/// Maximum number of items to keep in memory per view
pub const MAX_ITEMS_IN_MEMORY: usize = 1000;

/// Minimum time between data refreshes to avoid excessive API calls
pub const MIN_REFRESH_INTERVAL: Duration = Duration::from_millis(100);

/// Data retention duration for cleanup - will be used when UI implements time-based filtering
#[allow(dead_code)]
pub const DATA_RETENTION_DURATION: Duration = Duration::from_secs(3600); // 1 hour

/// Trait for state management with performance optimizations - will be used for memory management
#[allow(dead_code)]
pub trait StateManager {
    /// Apply pagination to limit items in memory
    fn apply_pagination(&mut self);

    /// Clean up old data based on retention policy
    fn cleanup_old_data(&mut self);

    /// Get the number of items currently in memory
    fn item_count(&self) -> usize;
}

/// Helper to track last update time for debouncing
#[derive(Debug, Clone)]
pub struct UpdateTracker {
    last_update: Instant,
    min_interval: Duration,
}

impl UpdateTracker {
    /// Create a new update tracker
    pub fn new(min_interval: Duration) -> Self {
        Self {
            last_update: Instant::now() - min_interval, // Allow immediate first update
            min_interval,
        }
    }

    /// Check if enough time has passed since last update
    pub fn should_update(&self) -> bool {
        self.last_update.elapsed() >= self.min_interval
    }

    /// Mark that an update has occurred
    pub fn mark_updated(&mut self) {
        self.last_update = Instant::now();
    }

    /// Get time since last update
    #[allow(dead_code)]
    pub fn time_since_update(&self) -> Duration {
        self.last_update.elapsed()
    }
}

/// Cache for API responses to reduce redundant requests
#[derive(Debug, Clone)]
pub struct ResponseCache<T> {
    data: Option<T>,
    cached_at: Instant,
    ttl: Duration,
}

#[allow(dead_code)]
impl<T: Clone> ResponseCache<T> {
    /// Create a new response cache with TTL
    pub fn new(ttl: Duration) -> Self {
        Self {
            data: None,
            cached_at: Instant::now(),
            ttl,
        }
    }

    /// Get cached data if still valid
    pub fn get(&self) -> Option<&T> {
        if self.is_valid() {
            self.data.as_ref()
        } else {
            None
        }
    }

    /// Store data in cache
    pub fn set(&mut self, data: T) {
        self.data = Some(data);
        self.cached_at = Instant::now();
    }

    /// Check if cached data is still valid
    pub fn is_valid(&self) -> bool {
        self.data.is_some() && self.cached_at.elapsed() < self.ttl
    }

    /// Clear the cache
    pub fn clear(&mut self) {
        self.data = None;
    }
}

/// Efficient data structure for managing large lists with pagination
#[derive(Debug, Clone)]
pub struct PaginatedList<T> {
    items: Vec<T>,
    max_items: usize,
}

#[allow(dead_code)]
impl<T> PaginatedList<T> {
    /// Create a new paginated list
    pub fn new(max_items: usize) -> Self {
        Self {
            items: Vec::with_capacity(max_items),
            max_items,
        }
    }

    /// Add items, automatically trimming to max size
    pub fn extend(&mut self, new_items: Vec<T>) {
        self.items.extend(new_items);
        self.trim_to_max();
    }

    /// Replace all items
    pub fn replace(&mut self, new_items: Vec<T>) {
        self.items = new_items;
        self.trim_to_max();
    }

    /// Get all items
    pub fn items(&self) -> &[T] {
        &self.items
    }

    /// Get mutable reference to items
    pub fn items_mut(&mut self) -> &mut Vec<T> {
        &mut self.items
    }

    /// Get number of items
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Clear all items
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Trim to maximum size, keeping most recent items
    fn trim_to_max(&mut self) {
        if self.items.len() > self.max_items {
            let excess = self.items.len() - self.max_items;
            self.items.drain(0..excess);
        }
    }
}

impl<T> Default for PaginatedList<T> {
    fn default() -> Self {
        Self::new(MAX_ITEMS_IN_MEMORY)
    }
}

// Made with Bob

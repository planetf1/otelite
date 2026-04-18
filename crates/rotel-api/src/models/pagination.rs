//! Pagination metadata for API responses

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Pagination metadata included in list responses
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PaginationMetadata {
    /// Total number of items available
    pub total: usize,

    /// Number of items returned in this response
    pub count: usize,

    /// Offset of the first item in this response
    pub offset: usize,

    /// Maximum number of items per page
    pub limit: usize,

    /// Offset for the next page (None if this is the last page)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<usize>,

    /// Offset for the previous page (None if this is the first page)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_offset: Option<usize>,
}

impl PaginationMetadata {
    /// Create pagination metadata from query results
    ///
    /// # Arguments
    /// * `total` - Total number of items available
    /// * `offset` - Current offset
    /// * `limit` - Maximum items per page
    /// * `count` - Number of items in current response
    pub fn new(total: usize, offset: usize, limit: usize, count: usize) -> Self {
        let next_offset = if offset + count < total {
            Some(offset + limit)
        } else {
            None
        };

        let prev_offset = if offset > 0 {
            Some(offset.saturating_sub(limit))
        } else {
            None
        };

        Self {
            total,
            count,
            offset,
            limit,
            next_offset,
            prev_offset,
        }
    }

    /// Check if there are more pages available
    pub fn has_next(&self) -> bool {
        self.next_offset.is_some()
    }

    /// Check if there are previous pages available
    pub fn has_prev(&self) -> bool {
        self.prev_offset.is_some()
    }

    /// Calculate the current page number (1-based)
    pub fn current_page(&self) -> usize {
        (self.offset / self.limit) + 1
    }

    /// Calculate the total number of pages
    pub fn total_pages(&self) -> usize {
        self.total.div_ceil(self.limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pagination_first_page() {
        let pagination = PaginationMetadata::new(100, 0, 10, 10);

        assert_eq!(pagination.total, 100);
        assert_eq!(pagination.count, 10);
        assert_eq!(pagination.offset, 0);
        assert_eq!(pagination.limit, 10);
        assert_eq!(pagination.next_offset, Some(10));
        assert_eq!(pagination.prev_offset, None);
        assert!(pagination.has_next());
        assert!(!pagination.has_prev());
        assert_eq!(pagination.current_page(), 1);
        assert_eq!(pagination.total_pages(), 10);
    }

    #[test]
    fn test_pagination_middle_page() {
        let pagination = PaginationMetadata::new(100, 50, 10, 10);

        assert_eq!(pagination.next_offset, Some(60));
        assert_eq!(pagination.prev_offset, Some(40));
        assert!(pagination.has_next());
        assert!(pagination.has_prev());
        assert_eq!(pagination.current_page(), 6);
    }

    #[test]
    fn test_pagination_last_page() {
        let pagination = PaginationMetadata::new(100, 90, 10, 10);

        assert_eq!(pagination.next_offset, None);
        assert_eq!(pagination.prev_offset, Some(80));
        assert!(!pagination.has_next());
        assert!(pagination.has_prev());
        assert_eq!(pagination.current_page(), 10);
    }

    #[test]
    fn test_pagination_partial_last_page() {
        let pagination = PaginationMetadata::new(95, 90, 10, 5);

        assert_eq!(pagination.count, 5);
        assert_eq!(pagination.next_offset, None);
        assert!(!pagination.has_next());
    }
}

// Made with Bob

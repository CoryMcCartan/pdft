use image::DynamicImage;
use lru::LruCache;
use std::num::NonZeroUsize;

/// Key for the rendered image cache.
/// Includes target dimensions so layout changes cause re-renders at the right resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    doc_id: usize,
    page_num: usize,
    max_w: u16,
    max_h: u16,
}

/// LRU cache of rendered page images.
/// Avoids re-rendering pages that have already been visited at the same resolution.
pub struct ImageCache {
    cache: LruCache<CacheKey, DynamicImage>,
}

impl ImageCache {
    /// Create a new cache holding up to `capacity` rendered images.
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: LruCache::new(NonZeroUsize::new(capacity).unwrap()),
        }
    }

    /// Get a cached image if available at the given resolution.
    pub fn get(&mut self, doc_id: usize, page_num: usize, max_w: u16, max_h: u16) -> Option<&DynamicImage> {
        self.cache.get(&CacheKey { doc_id, page_num, max_w, max_h })
    }

    /// Store a rendered image in the cache.
    pub fn put(&mut self, doc_id: usize, page_num: usize, max_w: u16, max_h: u16, image: DynamicImage) {
        self.cache.put(CacheKey { doc_id, page_num, max_w, max_h }, image);
    }

    /// Clear all cached images.
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

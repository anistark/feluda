use crate::licenses::License;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Thread-safe cache for GitHub licenses
pub struct LicenseCache {
    licenses: Arc<Mutex<Option<HashMap<String, License>>>>,
}

impl LicenseCache {
    /// Create a new empty license cache
    pub fn new() -> Self {
        Self {
            licenses: Arc::new(Mutex::new(None)),
        }
    }

    /// TODO: Set the cached licenses. Will be used when implementing cache invalidation
    /// or manual cache clearing features for the API.
    #[allow(dead_code)]
    pub fn set(&self, licenses: HashMap<String, License>) {
        if let Ok(mut cache) = self.licenses.lock() {
            *cache = Some(licenses);
        }
    }

    /// TODO: Get a reference to the cached licenses. Will be used for cache introspection
    /// features to allow users to query what's currently cached.
    #[allow(dead_code)]
    pub fn get(&self) -> Option<HashMap<String, License>> {
        if let Ok(cache) = self.licenses.lock() {
            cache.clone()
        } else {
            None
        }
    }

    /// TODO: Check if licenses are cached. Will be used to optimize repeated calls
    /// or implement lazy-loading patterns in future versions.
    #[allow(dead_code)]
    pub fn is_cached(&self) -> bool {
        if let Ok(cache) = self.licenses.lock() {
            cache.is_some()
        } else {
            false
        }
    }
}

impl Clone for LicenseCache {
    fn clone(&self) -> Self {
        Self {
            licenses: Arc::clone(&self.licenses),
        }
    }
}

impl Default for LicenseCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_license_cache_creation() {
        let cache = LicenseCache::new();
        assert!(!cache.is_cached());
    }

    #[test]
    fn test_license_cache_set_and_get() {
        let cache = LicenseCache::new();
        let mut licenses = HashMap::new();
        licenses.insert(
            "MIT".to_string(),
            License {
                key: "mit".to_string(),
                name: "MIT License".to_string(),
                spdx_id: "MIT".to_string(),
                url: "https://opensource.org/licenses/MIT".to_string(),
                html_url: "https://github.com/licenses/MIT".to_string(),
                description: "A short and simple permissive license with conditions only requiring preservation of copyright and license notices.".to_string(),
                implementation: "Add one or more copies of the notice, in text form, to any redistributed or derivative code.".to_string(),
                permissions: vec!["commercial-use".to_string()],
                conditions: vec!["include-copyright".to_string()],
                limitations: vec!["liability".to_string()],
            },
        );

        cache.set(licenses.clone());
        assert!(cache.is_cached());

        let retrieved = cache.get();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().len(), 1);
    }

    #[test]
    fn test_license_cache_clone() {
        let cache1 = LicenseCache::new();
        let cache2 = cache1.clone();

        let mut licenses = HashMap::new();
        licenses.insert("MIT".to_string(), License {
            key: "mit".to_string(),
            name: "MIT License".to_string(),
            spdx_id: "MIT".to_string(),
            url: "https://opensource.org/licenses/MIT".to_string(),
            html_url: "https://github.com/licenses/MIT".to_string(),
            description: "A short and simple permissive license.".to_string(),
            implementation: "Add one or more copies of the notice.".to_string(),
            permissions: vec![],
            conditions: vec![],
            limitations: vec![],
        });

        cache1.set(licenses);

        // Both caches should see the same data
        assert!(cache1.is_cached());
        assert!(cache2.is_cached());
    }
}

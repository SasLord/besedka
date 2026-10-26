//! `TtlCache<V>` — small hand-rolled TTL cache (no `moka`/`cached` crate).
//!
//! Ported from trackly `ad/cache.rs`. LAN scale, single map, no eviction
//! policy needed — a `Mutex<HashMap>` is the right amount of machinery.
//!
//! `RealAdDirectory` uses TWO instances: `display_name` (long TTL, cosmetic)
//! and `role` (short TTL — it gates authorization).
//!
//! Callers MUST normalize the key (lowercase, `@domain`/`DOMAIN\` stripped)
//! before `get`/`put`, so all three spellings of one identity share an entry.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Generic TTL-bounded in-memory cache. `Instant` (monotonic, process-local)
/// is deliberate — this is ephemeral state, not persisted time.
pub struct TtlCache<V: Clone> {
    entries: Mutex<HashMap<String, (V, Instant)>>,
    ttl: Duration,
}

impl<V: Clone> TtlCache<V> {
    /// New cache with a fixed TTL applied to every `put`.
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// Look up `key`. `None` if never put, or if its TTL expired (expired
    /// entries are simply not returned — no eager eviction at this scale).
    pub fn get(&self, key: &str) -> Option<V> {
        let map = self.entries.lock().expect("TtlCache mutex poisoned");
        map.get(key)
            .filter(|(_, expires_at)| *expires_at > Instant::now())
            .map(|(value, _)| value.clone())
    }

    /// Insert/overwrite `key`, resetting its expiry to `now + ttl`.
    pub fn put(&self, key: String, value: V) {
        let mut map = self.entries.lock().expect("TtlCache mutex poisoned");
        map.insert(key, (value, Instant::now() + self.ttl));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_then_get_returns_value() {
        let cache: TtlCache<String> = TtlCache::new(Duration::from_secs(60));
        cache.put("us100".to_string(), "Иванов Иван Иванович".to_string());
        assert_eq!(cache.get("us100"), Some("Иванов Иван Иванович".to_string()));
    }

    #[test]
    fn get_on_empty_cache_returns_none() {
        let cache: TtlCache<String> = TtlCache::new(Duration::from_secs(60));
        assert_eq!(cache.get("us200"), None);
    }

    #[test]
    fn entry_expires_after_ttl() {
        let cache: TtlCache<String> = TtlCache::new(Duration::from_millis(10));
        cache.put("us100".to_string(), "Иванов Иван Иванович".to_string());
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(cache.get("us100"), None);
    }

    #[test]
    fn distinct_keys_do_not_interfere() {
        let cache: TtlCache<i32> = TtlCache::new(Duration::from_secs(60));
        cache.put("a".to_string(), 1);
        cache.put("b".to_string(), 2);
        assert_eq!(cache.get("a"), Some(1));
        assert_eq!(cache.get("b"), Some(2));
    }
}

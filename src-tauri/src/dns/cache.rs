use super::*;
use crate::config::CoreConfig;
use moka::sync::Cache;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(crate) struct DnsCache {
    enabled: bool,
    max_entry_size: usize,
    min_ttl: u32,
    max_ttl: u32,
    negative_ttl: u32,
    pub(crate) inner: Cache<CacheKey, Arc<CacheEntry>>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) struct CacheKey {
    pub(crate) qname: String,
    pub(crate) qtype: u16,
    pub(crate) qclass: u16,
    pub(crate) route_id: i32,
}

#[derive(Clone)]
pub(crate) struct CacheEntry {
    pub(crate) response: Vec<u8>,
    pub(crate) expires_at: Instant,
    pub(crate) original_ttls: Vec<u32>,
    pub(crate) ttl_offsets: Vec<usize>,
    pub(crate) upstream_name: String,
    pub(crate) upstream_protocol: String,
    // Answer summary is parsed once here, at insert time, and shared by reference on every
    // cache hit. This keeps the hot read path free of DNS message parsing.
    pub(crate) answers: Arc<str>,
}

pub(crate) struct CachedResponse {
    pub(crate) response: Vec<u8>,
    pub(crate) upstream_name: String,
    pub(crate) upstream_protocol: String,
    pub(crate) answers: Arc<str>,
}

impl DnsCache {
    pub(crate) fn new(cfg: &CoreConfig) -> Self {
        Self {
            enabled: cfg.cache.enabled,
            max_entry_size: cfg.cache.max_entry_size,
            min_ttl: cfg.cache.min_ttl,
            max_ttl: cfg.cache.max_ttl,
            negative_ttl: cfg.cache.negative_ttl,
            inner: Cache::builder()
                .max_capacity(cfg.cache.max_entries.max(1) as u64)
                .build(),
        }
    }

    pub(crate) fn clear(&self) -> usize {
        self.inner.run_pending_tasks();
        let len = self.inner.entry_count() as usize;
        self.inner.invalidate_all();
        self.inner.run_pending_tasks();
        len
    }

    pub(crate) fn get(&self, key: &CacheKey, id: u16) -> Option<CachedResponse> {
        if !self.enabled {
            return None;
        }
        let now = Instant::now();
        let entry = self.inner.get(key)?;
        if now >= entry.expires_at {
            self.inner.invalidate(key);
            return None;
        }
        // `entry` is an `Arc<CacheEntry>`, so this only clones the response bytes we must
        // mutate before sending. TTL vectors and the answer summary stay behind the Arc.
        let mut resp = entry.response.clone();
        set_id(&mut resp, id);
        let remaining = entry
            .expires_at
            .saturating_duration_since(now)
            .as_secs()
            .min(u32::MAX as u64) as u32;
        if !entry.original_ttls.is_empty() {
            let _ = rewrite_ttls(
                &mut resp,
                &entry.original_ttls,
                &entry.ttl_offsets,
                remaining,
            );
        }
        Some(CachedResponse {
            response: resp,
            upstream_name: entry.upstream_name.clone(),
            upstream_protocol: entry.upstream_protocol.clone(),
            answers: entry.answers.clone(),
        })
    }

    pub(crate) fn set(
        &self,
        key: CacheKey,
        resp: &[u8],
        negative: bool,
        upstream_name: &str,
        upstream_protocol: &str,
    ) {
        if !self.enabled || resp.len() > self.max_entry_size {
            return;
        }
        let (original_ttls, ttl_offsets) = if negative {
            (Vec::new(), Vec::new())
        } else {
            rr_ttls_with_offsets(resp).unwrap_or_default()
        };
        let ttl = if negative && self.negative_ttl > 0 {
            self.negative_ttl
        } else {
            clamp_ttl(
                min_ttl_from_values(&original_ttls),
                self.min_ttl,
                self.max_ttl,
            )
        };
        if ttl == 0 {
            return;
        }
        // Parse the answer summary exactly once, here at insert time. Every subsequent cache
        // hit reuses this shared `Arc<str>` without touching the DNS wire format again.
        let answers = if negative {
            Arc::from("")
        } else {
            summarize_answers(resp)
        };
        self.inner.insert(
            key,
            Arc::new(CacheEntry {
                response: resp.to_vec(),
                expires_at: Instant::now() + Duration::from_secs(ttl as u64),
                original_ttls,
                ttl_offsets,
                upstream_name: upstream_name.to_string(),
                upstream_protocol: upstream_protocol.to_string(),
                answers,
            }),
        );
    }
}

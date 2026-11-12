//! Optional suppression of immediately repeated fixed-function capability calls.
//! Guest state bookkeeping still runs for every call; only the duplicate
//! desktop-driver submission is skipped. The cache is deliberately limited to
//! consecutive Enable/Disable pairs so it never becomes a second GL state
//! authority.

#[derive(Default)]
pub(crate) struct Cache {
    enabled: bool,
    last: Option<(u32, bool)>,
    hits: u64,
    forwards: u64,
}

impl Cache {
    pub(crate) fn new() -> Self {
        let enabled = crate::ds_draw_env::var("TOUCHHLE_DS_GL_STATE_CACHE").as_deref() == Ok("1");
        if enabled {
            eprintln!("[DSgl-state-cache] effective=consecutive-enable-disable");
        }
        Self {
            enabled,
            ..Self::default()
        }
    }

    pub(crate) fn suppress(&mut self, cap: u32, on: bool) -> bool {
        if !self.enabled {
            return false;
        }
        let duplicate = self.last == Some((cap, on));
        self.last = Some((cap, on));
        if duplicate {
            self.hits = self.hits.saturating_add(1);
        } else {
            self.forwards = self.forwards.saturating_add(1);
        }
        if (self.hits + self.forwards) % 100_000 == 0 {
            eprintln!(
                "[DSgl-state-cache] calls={} hits={} forwards={}",
                self.hits + self.forwards,
                self.hits,
                self.forwards
            );
        }
        duplicate
    }
}

#[cfg(test)]
mod tests {
    use super::Cache;

    #[test]
    fn disabled_cache_never_suppresses() {
        let mut cache = Cache::default();
        assert!(!cache.suppress(0xbe2, true));
        assert!(!cache.suppress(0xbe2, true));
    }

    #[test]
    fn only_immediately_repeated_operation_is_suppressed() {
        let mut cache = Cache {
            enabled: true,
            ..Cache::default()
        };
        assert!(!cache.suppress(0xbe2, true));
        assert!(cache.suppress(0xbe2, true));
        assert!(!cache.suppress(0xbe2, false));
        assert!(!cache.suppress(0xbe2, true));
        assert!(cache.suppress(0xbe2, true));
    }
}

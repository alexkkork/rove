//! Live Roblox client version.
//!
//! The gamejoin endpoint validates the `User-Agent`, so the version in it has
//! to track the real deployment instead of a pinned constant. The version is
//! read once from Roblox's own client-settings service, cached for the process
//! and refreshed if a join is rejected.
//!
//! Everything here is best-effort: any failure falls back to the compiled-in
//! default so a join never depends on this succeeding.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

/// Platform the UA claims. The wire protocol here is the desktop client's, and
/// Windows is the reference implementation for it.
const PLATFORM: &str = "WindowsPlayer";
const ENDPOINT_HOST: &str = "clientsettings.roblox.com";

/// Used until a live fetch succeeds, and on every later failure.
pub const FALLBACK_VERSION: &str = "0.748.0.7481526";

/// How long a fetched version stays good before another attempt.
const TTL: Duration = Duration::from_secs(6 * 60 * 60);
/// A failed fetch is retried sooner than a good one expires.
const FAILURE_TTL: Duration = Duration::from_secs(60);

struct Cached {
    version: String,
    fetched: Instant,
    ok: bool,
}

fn cache() -> &'static Mutex<Option<Cached>> {
    static CACHE: OnceLock<Mutex<Option<Cached>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Plausible `MAJOR.MINOR.PATCH.BUILD` version string, bounded.
pub fn valid_version(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 32
        && text.split('.').count() == 4
        && text
            .split('.')
            .all(|part| !part.is_empty() && part.len() <= 9 && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Full `User-Agent` for a Roblox desktop client on this platform.
pub fn user_agent() -> String {
    format!(
        "Roblox/WinInet RobloxApp/{} ({})",
        current_version(),
        PLATFORM
    )
}

/// Current best-known version: live if fetched, else the fallback.
pub fn current_version() -> String {
    let fresh = {
        let guard = cache().lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().filter(|entry| {
            entry.fetched.elapsed() < if entry.ok { TTL } else { FAILURE_TTL }
        })
        .map(|entry| entry.version.clone())
    };
    if let Some(version) = fresh {
        return version;
    }
    // Refreshing here keeps `user_agent()` synchronous at the call site.
    let fetched = fetch();
    let version = fetched.clone().unwrap_or_else(|| FALLBACK_VERSION.to_owned());
    let mut guard = cache().lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(Cached {
        version: version.clone(),
        fetched: Instant::now(),
        ok: fetched.is_some(),
    });
    version
}

/// Drop the cached version so the next call refetches. Used when a join is
/// rejected, in case the deployment moved.
pub fn invalidate() {
    *cache().lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Seed the cache directly (tests, or a version learned elsewhere).
pub fn seed(version: &str) {
    if !valid_version(version) {
        return;
    }
    *cache().lock().unwrap_or_else(|e| e.into_inner()) = Some(Cached {
        version: version.to_owned(),
        fetched: Instant::now(),
        ok: true,
    });
}

fn fetch() -> Option<String> {
    let url = format!("https://{ENDPOINT_HOST}/v2/client-version/{PLATFORM}");
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(6))
        .build();
    let response = agent.get(url.as_str()).call().ok()?;
    let body: Value = serde_json::from_reader(response.into_reader()).ok()?;
    let version = body.get("version").and_then(Value::as_str)?.to_owned();
    valid_version(&version).then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_shape_is_strict() {
        assert!(valid_version("0.748.0.7481526"));
        assert!(!valid_version("0.748"));
        assert!(!valid_version("0.748.0."));
        assert!(!valid_version(""));
        assert!(!valid_version("0.748.0.7481526\n"));
        assert!(!valid_version(&"1".repeat(40)));
        assert!(!valid_version("0.748.x.1"));
    }

    #[test]
    fn seed_overrides_and_invalidate_restores_fallback() {
        seed("0.999.0.9990001");
        assert_eq!(current_version(), "0.999.0.9990001");
        assert!(user_agent().contains("0.999.0.9990001"));
        invalidate();
        // Whatever the network did, we get a valid version back.
        assert!(valid_version(&current_version()));
    }
}

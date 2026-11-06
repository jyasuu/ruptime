//! In-memory state for passive "push" monitors (Uptime Kuma "Push" type).
//!
//! The monitored job calls `GET|POST /api/push/{token}?status=up&msg=OK&ping=12`.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct PushRecord {
    pub at: Instant,
    pub up: bool,
    pub message: String,
    pub ping_ms: Option<u128>,
}

static PUSHES: OnceLock<Mutex<HashMap<String, PushRecord>>> = OnceLock::new();
static STARTED: OnceLock<Instant> = OnceLock::new();

fn store() -> &'static Mutex<HashMap<String, PushRecord>> {
    PUSHES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Process start reference used for the "waiting for first push" grace period.
pub fn started_at() -> Instant {
    *STARTED.get_or_init(Instant::now)
}

pub fn record_push(token: &str, up: bool, message: String, ping_ms: Option<u128>) {
    if let Ok(mut m) = store().lock() {
        m.insert(
            token.to_string(),
            PushRecord {
                at: Instant::now(),
                up,
                message,
                ping_ms,
            },
        );
    }
}

pub fn last_push(token: &str) -> Option<PushRecord> {
    store().lock().ok().and_then(|m| m.get(token).cloned())
}

static KNOWN: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();

/// Registers a token configured by a `Push` check. The HTTP endpoint only
/// accepts registered tokens, so arbitrary tokens cannot grow memory.
pub fn register_token(token: &str) {
    if let Ok(mut k) = KNOWN.get_or_init(|| Mutex::new(Default::default())).lock() {
        k.insert(token.to_string());
    }
}

pub fn is_registered(token: &str) -> bool {
    KNOWN
        .get_or_init(|| Mutex::new(Default::default()))
        .lock()
        .map(|k| k.contains(token))
        .unwrap_or(false)
}

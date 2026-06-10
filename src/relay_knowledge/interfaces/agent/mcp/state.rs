use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    fmt,
    sync::{Arc, Mutex},
    time::Instant,
};

use axum::http::HeaderMap;
use tokio::sync::watch;

use super::MCP_SESSION_ID_HEADER;

const MAX_TRACKED_SESSIONS: usize = 1024;

#[derive(Clone)]
pub(super) struct SessionRegistry {
    active: Arc<Mutex<SessionState>>,
}

struct SessionState {
    issued: HashMap<String, SessionRecord>,
    usage_order: VecDeque<SessionUse>,
    next_revision: u64,
}

#[derive(Clone, Copy)]
struct SessionRecord {
    initialized: bool,
    last_used: u64,
    created_at: Instant,
    cold_start_recorded: bool,
}

struct SessionUse {
    session_id: String,
    revision: u64,
}

#[derive(Debug)]
pub(super) struct SessionLookup {
    session_id: String,
    pub(super) initialized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SessionLookupError {
    Missing,
    InvalidHeader,
    Unknown,
}

impl fmt::Display for SessionLookupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(formatter, "missing MCP session id"),
            Self::InvalidHeader => write!(formatter, "invalid MCP session id header"),
            Self::Unknown => write!(formatter, "unknown MCP session id"),
        }
    }
}

impl Error for SessionLookupError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SessionCreateError {
    EntropyUnavailable,
}

impl fmt::Display for SessionCreateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EntropyUnavailable => write!(formatter, "OS session entropy is unavailable"),
        }
    }
}

impl Error for SessionCreateError {}

impl SessionLookup {
    pub(super) fn namespace(&self) -> String {
        format!("session:{}", self.session_id)
    }

    pub(super) fn session_id(&self) -> &str {
        &self.session_id
    }
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self {
            active: Arc::new(Mutex::new(SessionState {
                issued: HashMap::new(),
                usage_order: VecDeque::new(),
                next_revision: 1,
            })),
        }
    }
}

impl SessionRegistry {
    pub(super) fn create_session(&self) -> Result<String, SessionCreateError> {
        loop {
            let session_id = generate_session_id()?;
            let mut active = self
                .active
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if active.issued.contains_key(&session_id) {
                continue;
            }
            active.insert_session(session_id.clone(), false);
            active.evict_inactive_sessions();
            return Ok(session_id);
        }
    }

    pub(super) fn require_session(
        &self,
        headers: &HeaderMap,
    ) -> Result<SessionLookup, SessionLookupError> {
        let Some(value) = headers.get(MCP_SESSION_ID_HEADER) else {
            return Err(SessionLookupError::Missing);
        };
        let session_id = value
            .to_str()
            .map_err(|_| SessionLookupError::InvalidHeader)?;

        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let initialized = active
            .touch_session(session_id)
            .ok_or(SessionLookupError::Unknown)?
            .initialized;

        Ok(SessionLookup {
            session_id: session_id.to_owned(),
            initialized,
        })
    }

    pub(super) fn mark_initialized(&self, session_id: &str) -> Result<(), SessionLookupError> {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(record) = active.issued.get_mut(session_id) else {
            return Err(SessionLookupError::Unknown);
        };
        record.initialized = true;
        active.touch_session(session_id);
        Ok(())
    }

    pub(super) fn record_tools_list_cold_start(&self, session_id: &str) -> Option<u64> {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let record = active.issued.get_mut(session_id)?;
        if record.cold_start_recorded {
            return None;
        }
        record.cold_start_recorded = true;
        Some(u64::try_from(record.created_at.elapsed().as_millis()).unwrap_or(u64::MAX))
    }

    pub(super) fn terminate_session(&self, headers: &HeaderMap) -> Result<(), SessionLookupError> {
        let lookup = self.require_session(headers)?;
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .issued
            .remove(lookup.session_id());
        Ok(())
    }

    #[cfg(test)]
    fn contains_session(&self, session_id: &str) -> bool {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .issued
            .contains_key(session_id)
    }

    #[cfg(test)]
    fn tracked_len(&self) -> usize {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .issued
            .len()
    }

    #[cfg(test)]
    fn usage_history_len(&self) -> usize {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .usage_order
            .len()
    }
}

impl SessionState {
    fn insert_session(&mut self, session_id: String, initialized: bool) {
        let revision = self.next_session_revision();
        self.issued.insert(
            session_id.clone(),
            SessionRecord {
                initialized,
                last_used: revision,
                created_at: Instant::now(),
                cold_start_recorded: false,
            },
        );
        self.usage_order.push_back(SessionUse {
            session_id,
            revision,
        });
    }

    fn touch_session(&mut self, session_id: &str) -> Option<SessionRecord> {
        let revision = self.next_session_revision();
        let record = self.issued.get_mut(session_id)?;
        record.last_used = revision;
        let updated = *record;
        self.usage_order.push_back(SessionUse {
            session_id: session_id.to_owned(),
            revision,
        });
        self.compact_usage_history_if_needed();
        Some(updated)
    }

    fn next_session_revision(&mut self) -> u64 {
        let revision = self.next_revision;
        self.next_revision = self.next_revision.wrapping_add(1);
        revision
    }

    fn evict_inactive_sessions(&mut self) {
        while self.issued.len() > MAX_TRACKED_SESSIONS {
            let Some(candidate) = self.usage_order.pop_front() else {
                self.issued.clear();
                break;
            };
            if self
                .issued
                .get(&candidate.session_id)
                .is_some_and(|record| record.last_used == candidate.revision)
            {
                self.issued.remove(&candidate.session_id);
            }
        }
    }

    fn compact_usage_history_if_needed(&mut self) {
        if self.usage_order.len() <= MAX_TRACKED_SESSIONS * 2 {
            return;
        }

        let mut current_uses = self
            .issued
            .iter()
            .map(|(session_id, record)| SessionUse {
                session_id: session_id.clone(),
                revision: record.last_used,
            })
            .collect::<Vec<_>>();
        current_uses.sort_by_key(|entry| entry.revision);
        self.usage_order = current_uses.into();
    }
}

fn generate_session_id() -> Result<String, SessionCreateError> {
    let mut entropy = [0_u8; 32];
    getrandom::getrandom(&mut entropy).map_err(|_| SessionCreateError::EntropyUnavailable)?;
    Ok(format!("rk-{}", lowercase_hex(&entropy)))
}

fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[derive(Clone, Default)]
pub(super) struct CancellationRegistry {
    active: Arc<Mutex<CancellationState>>,
}

#[derive(Default)]
struct CancellationState {
    entries: HashMap<String, CancellationEntry>,
    next_token: u64,
}

struct CancellationEntry {
    sender: watch::Sender<bool>,
    token: u64,
}

impl CancellationRegistry {
    pub(super) fn register(
        &self,
        request_id: String,
    ) -> (watch::Receiver<bool>, CancellationRegistration) {
        let (sender, receiver) = watch::channel(false);
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let token = active.next_token;
        active.next_token = active.next_token.wrapping_add(1);
        active
            .entries
            .insert(request_id.clone(), CancellationEntry { sender, token });

        (
            receiver,
            CancellationRegistration {
                registry: self.clone(),
                request_id,
                token,
            },
        )
    }

    pub(super) fn cancel(&self, request_id: &str) {
        if let Some(sender) = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .get(request_id)
            .map(|entry| entry.sender.clone())
        {
            let _ = sender.send(true);
        }
    }

    fn finish(&self, request_id: &str, token: u64) {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if active
            .entries
            .get(request_id)
            .is_some_and(|entry| entry.token == token)
        {
            active.entries.remove(request_id);
        }
    }

    #[cfg(test)]
    pub(super) fn active_len(&self) -> usize {
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .len()
    }
}

pub(super) struct CancellationRegistration {
    registry: CancellationRegistry,
    request_id: String,
    token: u64,
}

impl Drop for CancellationRegistration {
    fn drop(&mut self) {
        self.registry.finish(&self.request_id, self.token);
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;

    use super::*;

    #[test]
    fn session_ids_are_unpredictable_header_safe_values() {
        let sessions = SessionRegistry::default();
        let first = create_session(&sessions);
        let second = create_session(&sessions);

        assert_ne!(first, second);
        assert!(is_session_id(&first));
        assert!(is_session_id(&second));
    }

    #[test]
    fn issued_sessions_resolve_stable_namespaces_and_track_initialization() {
        let sessions = SessionRegistry::default();
        let session_id = create_session(&sessions);
        let headers = session_headers(&session_id);

        let before_initialized = sessions
            .require_session(&headers)
            .expect("issued session should resolve");
        assert_eq!(
            before_initialized.namespace(),
            format!("session:{session_id}")
        );
        assert!(!before_initialized.initialized);

        sessions
            .mark_initialized(before_initialized.session_id())
            .expect("session should initialize");
        let after_initialized = sessions
            .require_session(&headers)
            .expect("initialized session should resolve");
        assert!(after_initialized.initialized);
    }

    #[test]
    fn missing_or_unknown_session_headers_are_rejected() {
        let sessions = SessionRegistry::default();
        let unknown_headers = session_headers("rk-unissued");

        assert_eq!(
            sessions
                .require_session(&HeaderMap::new())
                .expect_err("missing header should fail"),
            SessionLookupError::Missing
        );
        assert_eq!(
            sessions
                .require_session(&unknown_headers)
                .expect_err("unknown header should fail"),
            SessionLookupError::Unknown
        );
    }

    #[test]
    fn session_eviction_preserves_recently_used_sessions() {
        let sessions = SessionRegistry::default();
        let active = create_session(&sessions);
        let stale = create_session(&sessions);
        for _ in 0..(MAX_TRACKED_SESSIONS - 2) {
            create_session(&sessions);
        }
        sessions
            .require_session(&session_headers(&active))
            .expect("active session should touch recency");

        let newest = create_session(&sessions);

        assert!(sessions.contains_session(&active));
        assert!(sessions.contains_session(&newest));
        assert!(!sessions.contains_session(&stale));
        assert_eq!(sessions.tracked_len(), MAX_TRACKED_SESSIONS);
    }

    #[test]
    fn session_usage_history_is_bounded_for_stable_sessions() {
        let sessions = SessionRegistry::default();
        let session_id = create_session(&sessions);
        let headers = session_headers(&session_id);

        for _ in 0..(MAX_TRACKED_SESSIONS * 3) {
            sessions
                .require_session(&headers)
                .expect("stable session should resolve");
        }

        assert!(sessions.usage_history_len() <= MAX_TRACKED_SESSIONS * 2);
    }

    fn create_session(sessions: &SessionRegistry) -> String {
        sessions
            .create_session()
            .expect("OS entropy should create session")
    }

    fn session_headers(session_id: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(MCP_SESSION_ID_HEADER, session_id.parse().unwrap());
        headers
    }

    fn is_session_id(value: &str) -> bool {
        value.len() == 67
            && value.starts_with("rk-")
            && value[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
    }
}

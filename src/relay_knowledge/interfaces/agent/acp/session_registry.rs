use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use tokio::sync::watch;

use crate::api::RuntimeIdentity;

#[derive(Clone, Default)]
pub(super) struct AcpSessionRegistry {
    inner: Arc<Mutex<AcpSessionState>>,
}

#[derive(Default)]
struct AcpSessionState {
    sessions: HashMap<String, AcpSessionRecord>,
    active_requests: HashMap<String, watch::Sender<bool>>,
}

#[derive(Debug, Clone)]
pub(super) struct AcpSessionRecord {
    client_name: Option<String>,
    client_version: Option<String>,
    actor_id: Option<String>,
}

impl AcpSessionRecord {
    pub(super) fn new(
        client_name: Option<String>,
        client_version: Option<String>,
        actor_id: Option<String>,
    ) -> Self {
        Self {
            client_name: normalized_optional(client_name),
            client_version: normalized_optional(client_version),
            actor_id: normalized_optional(actor_id),
        }
    }

    pub(super) fn identity(&self, session_id: &str, request_id: Option<String>) -> RuntimeIdentity {
        RuntimeIdentity::acp(
            self.client_name.clone(),
            self.client_version.clone(),
            self.actor_id.clone(),
            session_id.to_owned(),
            request_id,
        )
    }
}

pub(super) struct ActiveAcpRequest {
    registry: AcpSessionRegistry,
    key: String,
    released: bool,
}

impl ActiveAcpRequest {
    pub(super) fn release(mut self) {
        self.registry.remove_request(&self.key);
        self.released = true;
    }
}

impl Drop for ActiveAcpRequest {
    fn drop(&mut self) {
        if !self.released {
            self.registry.remove_request(&self.key);
        }
    }
}

impl AcpSessionRegistry {
    pub(super) fn insert_session(&self, session_id: String, record: AcpSessionRecord) {
        self.state().sessions.insert(session_id, record);
    }

    pub(super) fn session(&self, session_id: &str) -> Option<AcpSessionRecord> {
        self.state().sessions.get(session_id).cloned()
    }

    pub(super) fn register_request(
        &self,
        session_id: &str,
        request_id: String,
    ) -> (watch::Receiver<bool>, ActiveAcpRequest) {
        let (sender, receiver) = watch::channel(false);
        let key = active_request_key(session_id, &request_id);
        self.state().active_requests.insert(key.clone(), sender);

        (
            receiver,
            ActiveAcpRequest {
                registry: self.clone(),
                key,
                released: false,
            },
        )
    }

    pub(super) fn cancel_request(&self, session_id: &str, request_id: &str) -> bool {
        let key = active_request_key(session_id, request_id);
        self.state()
            .active_requests
            .get(&key)
            .is_some_and(|sender| sender.send(true).is_ok())
    }

    fn remove_request(&self, key: &str) {
        self.state().active_requests.remove(key);
    }

    fn state(&self) -> std::sync::MutexGuard<'_, AcpSessionState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn active_request_key(session_id: &str, request_id: &str) -> String {
    format!("{session_id}|{request_id}")
}

fn normalized_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    })
}

#[cfg(test)]
#[path = "session_registry_tests.rs"]
mod tests;

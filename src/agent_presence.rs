//! Request-scoped MCP activity. This does not create or retain MCP sessions.
use crate::{service::scope, storage::Scope, types::AuthContext};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub(crate) const ACTIVITY_WINDOW: Duration = Duration::from_secs(300);
const WORKSPACE_LIMIT: usize = 128;
const CREDENTIAL_LIMIT: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClientKind {
    Codex,
    ClaudeCode,
    Other,
}

impl ClientKind {
    pub(crate) fn from_client_name(name: &str) -> Self {
        if name.len() > 32 {
            return Self::Other;
        }
        match name.to_ascii_lowercase().as_str() {
            "codex" | "codex-cli" | "codex_cli_rs" | "codex-mcp-client" | "codex-tui" => {
                Self::Codex
            }
            "claude-code" | "claude code" => Self::ClaudeCode,
            _ => Self::Other,
        }
    }
}

#[derive(Default)]
pub(crate) struct AgentPresence {
    workspaces: Mutex<HashMap<Scope, HashMap<Uuid, Activity>>>,
}

struct Activity {
    last_seen: Instant,
    last_seen_at_ms: i64,
    active_requests: usize,
    client_kinds: Vec<ClientKind>,
}

impl Activity {
    fn visible_at(&self, now: Instant) -> bool {
        self.active_requests != 0 || now.duration_since(self.last_seen) < ACTIVITY_WINDOW
    }

    fn touch(&mut self) {
        self.last_seen = Instant::now();
        self.last_seen_at_ms = crate::storage::sqlite::now_ms();
    }
}

#[derive(Serialize)]
pub(crate) struct ObservedAgent {
    pub(crate) credential_id: Uuid,
    last_seen_at_ms: i64,
    active_requests: usize,
    client_kinds: Vec<ClientKind>,
}

pub(crate) struct AgentRequest<'a> {
    presence: &'a AgentPresence,
    scope: Scope,
    credential: Uuid,
}

impl AgentPresence {
    pub(crate) fn begin(
        &self,
        identity: &AuthContext,
        kind: Option<ClientKind>,
    ) -> Option<AgentRequest<'_>> {
        let mut workspaces = self.workspaces.lock().ok()?;
        let scope = scope(identity);
        let now = Instant::now();
        if !workspaces.contains_key(&scope) && workspaces.len() == WORKSPACE_LIMIT {
            workspaces.retain(|_, entries| entries.values().any(|e| e.visible_at(now)));
            if workspaces.len() == WORKSPACE_LIMIT {
                return None;
            }
        }
        let entries = workspaces.entry(scope).or_default();
        if !entries.contains_key(&identity.id) && entries.len() == CREDENTIAL_LIMIT {
            entries.retain(|_, entry| entry.visible_at(now));
            if entries.len() == CREDENTIAL_LIMIT {
                return None;
            }
        }
        let entry = entries.entry(identity.id).or_insert_with(|| Activity {
            last_seen: now,
            last_seen_at_ms: 0,
            active_requests: 0,
            client_kinds: Vec::new(),
        });
        if !entry.visible_at(now) {
            entry.client_kinds.clear();
        }
        entry.touch();
        entry.active_requests += 1;
        if let Some(kind) = kind
            && !entry.client_kinds.contains(&kind)
        {
            entry.client_kinds.push(kind);
            entry.client_kinds.sort_unstable();
        }
        Some(AgentRequest {
            presence: self,
            scope,
            credential: identity.id,
        })
    }

    pub(crate) fn snapshot(&self, scope: Scope) -> (Vec<ObservedAgent>, bool) {
        self.snapshot_at(scope, Instant::now())
    }

    fn snapshot_at(&self, scope: Scope, now: Instant) -> (Vec<ObservedAgent>, bool) {
        let Ok(mut workspaces) = self.workspaces.lock() else {
            return (Vec::new(), true);
        };
        let Some(entries) = workspaces.get_mut(&scope) else {
            return (Vec::new(), workspaces.len() == WORKSPACE_LIMIT);
        };
        entries.retain(|_, entry| entry.visible_at(now));
        let mut peers: Vec<_> = entries
            .iter()
            .map(|(&credential_id, entry)| ObservedAgent {
                credential_id,
                last_seen_at_ms: entry.last_seen_at_ms,
                active_requests: entry.active_requests,
                client_kinds: entry.client_kinds.clone(),
            })
            .collect();
        peers.sort_unstable_by_key(|peer| {
            (
                peer.active_requests == 0,
                std::cmp::Reverse(peer.last_seen_at_ms),
                peer.credential_id,
            )
        });
        (peers, entries.len() == CREDENTIAL_LIMIT)
    }
}

impl Drop for AgentRequest<'_> {
    fn drop(&mut self) {
        if let Ok(mut workspaces) = self.presence.workspaces.lock()
            && let Some(entry) = workspaces
                .get_mut(&self.scope)
                .and_then(|entries| entries.get_mut(&self.credential))
        {
            entry.active_requests -= 1;
            entry.touch();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_flight_requests_stay_visible_and_idle_credentials_expire() {
        let presence = AgentPresence::default();
        let identity = AuthContext {
            id: Uuid::new_v4(),
            tenant_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            role: "workspace".into(),
        };
        let first = presence.begin(&identity, Some(ClientKind::Codex)).unwrap();
        let parallel = presence.begin(&identity, None).unwrap();
        let (rows, _) = presence.snapshot(scope(&identity));
        assert_eq!(rows[0].active_requests, 2);
        assert_eq!(rows[0].client_kinds, [ClientKind::Codex]);
        drop(first);
        let future = Instant::now() + ACTIVITY_WINDOW + Duration::from_secs(1);
        assert_eq!(
            presence.snapshot_at(scope(&identity), future).0[0].active_requests,
            1
        );
        drop(parallel);
        assert_eq!(presence.snapshot(scope(&identity)).0[0].active_requests, 0);
        let future = Instant::now() + ACTIVITY_WINDOW;
        assert!(presence.snapshot_at(scope(&identity), future).0.is_empty());
    }
}

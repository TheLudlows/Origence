//! Opt-in private question/answer synchronization over Origence's archive API.
mod redact;

use super::{
    Client, client_id, derived_uuid, digest,
    plugin::{id, private_plugin_storage, project_root},
    queue::{
        lock_private, prepare_private_directory, read_private_state, try_sender_lock,
        write_private_state,
    },
};
use crate::{
    client::HostClient,
    error::AppError,
    session::{EvidenceUse, SessionTurnInput},
    types::AuthContext,
};
use anyhow::{Context, ensure};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, path::PathBuf, sync::Arc, time::Duration};
use uuid::Uuid;

const NETWORK_BUDGET: Duration = Duration::from_secs(2);
const STATE_LIMIT: u64 = 16 * 1024 * 1024;
const PENDING_LIMIT: usize = 16;
const RECENT_LIMIT: usize = 256;
const SEND_LIMIT: usize = 4;

#[derive(Deserialize)]
struct Hook {
    hook_event_name: String,
    agent_id: Option<String>,
    session_id: Option<String>,
    turn_id: Option<String>,
    prompt_id: Option<String>,
    prompt: Option<String>,
    last_assistant_message: Option<String>,
}

enum Event {
    Prompt(Uuid, Uuid, Option<String>),
    Stop(Uuid, Option<String>),
    Abandon(Uuid),
    SessionEnd(Uuid),
}

impl Hook {
    fn event(self, client: Client, token: &str) -> Option<Event> {
        if self.agent_id.is_some() {
            return None;
        }
        let session = client_id(self.session_id.as_deref()?)?;
        let session_key = || {
            derived_uuid(digest(
                b"origence-private-turn-sync-session-v1",
                [client.namespace(), session],
            ))
        };
        if self.hook_event_name == "SessionEnd" {
            return Some(Event::SessionEnd(session_key()));
        }
        let native_turn = match client {
            Client::Codex => self.turn_id.as_deref(),
            Client::ClaudeCode => self.prompt_id.as_deref(),
        };
        let native_turn = client_id(native_turn?)?;
        let turn = derived_uuid(digest(
            b"origence-private-turn-sync-id-v1",
            [client.namespace(), session, native_turn],
        ));
        Some(match self.hook_event_name.as_str() {
            "UserPromptSubmit" => Event::Prompt(
                session_key(),
                turn,
                self.prompt
                    .as_deref()
                    .and_then(|text| redact::text(text, token)),
            ),
            "Stop" => match self.last_assistant_message.as_deref() {
                None => Event::Stop(turn, None),
                Some(text) => redact::text(text, token).map_or(Event::Abandon(turn), |answer| {
                    Event::Stop(turn, Some(answer))
                }),
            },
            "Interrupt" | "StopFailure" => Event::Abandon(turn),
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Destination {
    tenant: Uuid,
    workspace: Uuid,
    logical_key: Uuid,
    session: Uuid,
}

impl Destination {
    async fn resolve(host: &HostClient) -> anyhow::Result<(HostClient, Self)> {
        let auth: AuthContext =
            serde_json::from_value(host.call(Method::GET, "/v1/whoami", None, None).await?)?;
        auth.require("write")?;
        ensure!(
            !auth.workspace_id.is_nil() && !auth.tenant_id.is_nil(),
            "invalid archive scope"
        );
        let host = host.clone().with_workspace(auth.workspace_id);
        let session = host.call(Method::GET, "/v1/session", None, None).await?;
        ensure!(
            id(&session["workspace_id"]) == Some(auth.workspace_id),
            "archive workspace changed"
        );
        let target = Self {
            tenant: auth.tenant_id,
            workspace: auth.workspace_id,
            logical_key: id(&session["key_id"]).context("missing logical key")?,
            session: id(&session["session_id"]).context("missing private session")?,
        };
        Ok((host, target))
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    target: Option<Destination>,
    prompts: VecDeque<Prompt>,
    pending: VecDeque<SessionTurnInput>,
    /// Bounded receipts and abandonment markers contain no conversation text.
    recent: VecDeque<(Uuid, Option<[u8; 32]>)>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Prompt {
    session: Uuid,
    turn: Uuid,
    text: String,
}

fn answer_hash(answer: &str) -> [u8; 32] {
    digest(b"origence-private-turn-sync-answer-v1", [answer.as_bytes()])
}

impl State {
    fn finish(&mut self, turn: Uuid, answer: Option<[u8; 32]>) {
        self.prompts.retain(|prompt| prompt.turn != turn);
        if self.recent.len() == RECENT_LIMIT {
            self.recent.pop_front();
        }
        self.recent.push_back((turn, answer));
    }

    fn observe(&mut self, event: Event) -> anyhow::Result<Option<&'static str>> {
        let turn = match &event {
            Event::Prompt(_, turn, _) | Event::Stop(turn, _) | Event::Abandon(turn) => *turn,
            Event::SessionEnd(session) => {
                self.prompts.retain(|prompt| prompt.session != *session);
                return Ok(None);
            }
        };

        let previous = self
            .pending
            .iter()
            .find(|entry| entry.turn_id == turn)
            .map(|entry| Some(answer_hash(&entry.answer)))
            .or_else(|| {
                self.recent
                    .iter()
                    .find(|(id, _)| *id == turn)
                    .map(|(_, hash)| *hash)
            });
        if let Some(previous) = previous {
            if let (Some(hash), Event::Stop(_, Some(answer))) = (previous, &event)
                && hash != answer_hash(answer)
            {
                return Ok(Some("completed turn changed; first answer kept"));
            }
            return Ok(None);
        }

        let prompt = self.prompts.iter().position(|prompt| prompt.turn == turn);
        match event {
            Event::Prompt(session, _, Some(text)) => {
                if let Some(index) = prompt {
                    if self.prompts[index].text != text {
                        return Ok(Some("prompt changed; first observation kept"));
                    }
                } else {
                    let notice = if self.prompts.len() == PENDING_LIMIT {
                        let first = self.prompts.front().context("missing unfinished turn")?;
                        self.finish(first.turn, None);
                        Some("an unfinished turn exceeded the local queue limit")
                    } else {
                        None
                    };
                    self.prompts.push_back(Prompt {
                        session,
                        turn,
                        text,
                    });
                    return Ok(notice);
                }
            }
            Event::Prompt(_, _, None) | Event::Abandon(_) => self.finish(turn, None),
            Event::Stop(_, Some(answer)) => {
                let Some(index) = prompt else {
                    return Ok(Some("matching prompt was unavailable"));
                };
                if self.pending.len() == PENDING_LIMIT {
                    return Ok(Some(
                        "completed turn could not enter the full archive queue",
                    ));
                }
                let question = self
                    .prompts
                    .remove(index)
                    .context("missing observed prompt")?
                    .text;
                let input = SessionTurnInput {
                    turn_id: turn,
                    question,
                    answer,
                    used_evidence: Vec::new(),
                    evidence_use: EvidenceUse::Unknown,
                };
                input.validate().map_err(anyhow::Error::msg)?;
                self.pending.push_back(input);
            }
            Event::Stop(_, None) => {
                // Some supported hosts can deliver a later Stop with the answer.
                return Ok(Some("completed answer was unavailable"));
            }
            Event::SessionEnd(_) => unreachable!(),
        }
        Ok(None)
    }

    fn acknowledge(&mut self, expected: Uuid) -> anyhow::Result<()> {
        let first = self.pending.front().context("archive queue changed")?;
        ensure!(first.turn_id == expected, "archive queue changed");
        let hash = answer_hash(&first.answer);
        self.pending.pop_front();
        self.finish(expected, Some(hash));
        Ok(())
    }
}

#[derive(Clone)]
struct Store(PathBuf);

impl Store {
    async fn blocking<R: Send + 'static>(
        &self,
        op: impl FnOnce(Self) -> anyhow::Result<R> + Send + 'static,
    ) -> anyhow::Result<R> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || op(store)).await?
    }

    fn read(&self) -> anyhow::Result<State> {
        let state: State = read_private_state(&self.0.join("turns-v1.json"), STATE_LIMIT)?;
        ensure!(
            state.prompts.len() <= PENDING_LIMIT
                && state.pending.len() <= PENDING_LIMIT
                && state.recent.len() <= RECENT_LIMIT,
            "archive sync state exceeds limits"
        );
        Ok(state)
    }

    fn update<R>(&self, op: impl FnOnce(&mut State) -> anyhow::Result<R>) -> anyhow::Result<R> {
        let _lock = lock_private(&self.0.join("state.lock"))?;
        let mut state = self.read()?;
        let result = op(&mut state)?;
        write_private_state(&self.0.join("turns-v1.json"), &state, STATE_LIMIT)?;
        Ok(result)
    }
}

/// Default-off and independent of Run metadata capture.
pub(super) async fn sync_plugin_turn(
    client: Client,
    origin: &str,
    workspace: Option<Uuid>,
    token: String,
    data: PathBuf,
    config: PathBuf,
    payload: Arc<[u8]>,
) -> anyhow::Result<Option<&'static str>> {
    let base = HostClient::parse_base(origin)?;
    ensure!(base.path() == "/", "archive server URL must be an origin");
    let client_token = token.clone();
    let origin_scope = base.to_string();
    let local = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        ensure!(config.is_absolute(), "archive project must be absolute");
        let project = config.canonicalize()?;
        let cwd = std::env::current_dir()?.canonicalize()?;
        if project_root(&cwd) != project {
            return Ok(None);
        }
        let hook: Hook = serde_json::from_slice(&payload)?;
        let Some(event) = hook.event(client, &client_token) else {
            return Ok(None);
        };
        let directory = private_plugin_storage(&data, &project)?;
        let scope = derived_uuid(digest(
            b"origence-private-turn-sync-scope-v1",
            [
                client.namespace(),
                project.as_os_str().as_encoded_bytes(),
                origin_scope.as_bytes(),
                workspace.unwrap_or_default().as_bytes(),
                client_token.as_bytes(),
            ],
        ));
        let store = Store(directory.join(format!("turns-{scope}.queue")));
        prepare_private_directory(&store.0)?;
        Ok(Some((store, event)))
    })
    .await??;
    let Some((store, event)) = local else {
        return Ok(None);
    };
    let host = HostClient::with_timeout(base, token, NETWORK_BUDGET)?;
    let host = if let Some(workspace) = workspace {
        host.with_workspace(workspace)
    } else {
        host
    };
    run(&store, host, workspace, event).await
}

async fn run(
    store: &Store,
    host: HostClient,
    requested_workspace: Option<Uuid>,
    event: Event,
) -> anyhow::Result<Option<&'static str>> {
    let stored = store.blocking(|s| Ok(s.read()?.target)).await?;
    let target = match stored {
        Some(target) => target,
        None if matches!(event, Event::Prompt(_, _, Some(_))) => {
            let (_, target) =
                tokio::time::timeout(NETWORK_BUDGET, Destination::resolve(&host)).await??;
            ensure!(
                requested_workspace.is_none_or(|expected| expected == target.workspace),
                "archive workspace does not match the configured workspace"
            );
            target
        }
        None => return Ok(None),
    };

    let delivery = matches!(event, Event::Stop(..) | Event::SessionEnd(_));
    // Capture locally before making network calls. A failed Stop must not lose
    // already queued turns or prevent an attempt to deliver them.
    let observed = store
        .blocking(move |s| {
            s.update(|state| {
                ensure!(
                    state.target.is_none_or(|bound| bound == target),
                    "archive scope changed"
                );
                state.target = Some(target);
                state.observe(event)
            })
        })
        .await;
    if !delivery {
        return observed;
    }
    let outcome = tokio::time::timeout(NETWORK_BUDGET, deliver(store, host, target)).await;
    match observed {
        Ok(Some(notice)) => Ok(Some(notice)),
        Err(_) => Ok(Some("a turn could not be recorded")),
        Ok(None) => match outcome {
            Ok(Ok(notice)) => Ok(notice),
            _ => Ok(Some("delivery deferred")),
        },
    }
}

async fn deliver(
    store: &Store,
    host: HostClient,
    target: Destination,
) -> anyhow::Result<Option<&'static str>> {
    let sender = store
        .blocking(|store| {
            let Some(lock) = try_sender_lock(&store.0)? else {
                return Ok(None);
            };
            if store.read()?.pending.is_empty() {
                return Ok(None);
            }
            Ok(Some(lock))
        })
        .await?;
    let Some(_sender) = sender else {
        return Ok(None);
    };
    let (host, actual) = Destination::resolve(&host).await?;
    ensure!(target == actual, "archive scope changed");
    let path = format!("/v1/sessions/{}/turns", target.session);
    let mut notice = None;
    for _ in 0..SEND_LIMIT {
        let Some(input) = store
            .blocking(|s| Ok(s.read()?.pending.pop_front()))
            .await?
        else {
            break;
        };
        let turn = input.turn_id;
        let key = format!("turn-sync-{turn}");
        match host
            .call(
                Method::POST,
                &path,
                Some(&serde_json::to_value(input)?),
                Some(&key),
            )
            .await
        {
            Ok(receipt) => ensure!(
                receipt["state"] == "archived"
                    && id(&receipt["session_id"]) == Some(target.session)
                    && id(&receipt["turn_id"]) == Some(turn)
                    && id(&receipt["source_event_id"]).is_some(),
                "invalid archive receipt"
            ),
            Err(AppError::Conflict(_)) => notice = Some("archived turn conflict"),
            Err(AppError::NotFound) => notice = Some("archive source unavailable"),
            Err(err) => return Err(err.into()),
        }
        store
            .blocking(move |s| s.update(|state| state.acknowledge(turn)))
            .await?;
    }
    Ok(notice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_sync_session_end_clears_only_its_unfinished_prompts() {
        let mut state = State::default();
        let session = Uuid::new_v4();
        let other = Uuid::new_v4();
        let complete = Uuid::new_v4();
        let unfinished = Uuid::new_v4();
        let unrelated = Uuid::new_v4();
        for (owner, turn) in [
            (session, complete),
            (session, unfinished),
            (other, unrelated),
        ] {
            assert_eq!(
                state
                    .observe(Event::Prompt(owner, turn, Some("question".into())))
                    .unwrap(),
                None
            );
        }
        state
            .observe(Event::Stop(complete, Some("answer".into())))
            .unwrap();
        state.observe(Event::SessionEnd(session)).unwrap();
        assert_eq!(state.prompts.front().unwrap().turn, unrelated);
        assert_eq!(state.prompts.len(), 1);
        assert_eq!(state.pending.front().unwrap().turn_id, complete);
        assert_eq!(
            state
                .observe(Event::Stop(unfinished, Some("delayed answer".into())))
                .unwrap(),
            Some("matching prompt was unavailable")
        );
        state.observe(Event::Abandon(unrelated)).unwrap();
        state
            .observe(Event::Stop(unrelated, Some("replayed after abort".into())))
            .unwrap();
        assert_eq!(state.pending.len(), 1);
    }
}

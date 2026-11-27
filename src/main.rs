use clap::{Parser, Subcommand};
use origence::{
    client::HostClient,
    client_hook::{self, Client as HookClient, Profile as HookProfile},
    client_skill::{SkillClient, export_skill},
    host,
    models::Models,
    service::Service,
    storage::{
        KeyCapabilities, Lifecycle, RelationalStore, Scope, SqliteStore, anyseek::AnySeekConfig,
    },
    types::{AuthContext, ResolveInput, SearchInput},
};
use serde_json::json;
use std::{future::Future, path::PathBuf, pin::Pin, time::Duration};
use uuid::Uuid;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[arg(long, env = "OC_DATA_DIR", default_value = ".data", global = true)]
    data_dir: PathBuf,
    #[arg(
        long,
        env = "OC_SERVER_URL",
        default_value = "http://127.0.0.1:8080",
        global = true
    )]
    server_url: String,
    /// Explicitly open local storage; requires exclusive access to the data directory.
    #[arg(long, global = true)]
    offline: bool,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Print the exact configuration for a fresh AnySeek lexical collection.
    AnyseekSchema {
        #[arg(long)]
        dimensions: Option<usize>,
        /// Include the configured embedding space alongside the collection config.
        #[arg(long)]
        with_space: bool,
    },
    /// Run the HTTP API and its single durable worker.
    #[command(alias = "api")]
    Serve {
        #[arg(long, env = "OC_BIND", default_value = "127.0.0.1:8080")]
        bind: String,
        /// Public HTTPS origin for /mcp behind a TLS-terminating proxy.
        #[arg(long, env = "OC_MCP_PUBLIC_ORIGIN")]
        mcp_public_origin: Option<String>,
        #[cfg(feature = "embedded-anyseek")]
        /// Open AnySeek in this process via its local-disk Rust SDK.
        #[arg(long)]
        local: bool,
    },
    /// Offline bootstrap: create a workspace and print its admin token once.
    WorkspaceCreate {
        name: String,
        /// Create in the tenant of this existing workspace.
        #[arg(long)]
        in_tenant_of: Option<Uuid>,
    },
    KeyCreate {
        workspace_id: Uuid,
        #[arg(long, default_value = "reader")]
        role: String,
        /// Allow this root key to access one layer of owned workspaces.
        #[arg(long)]
        allow_owned_workspaces: bool,
    },
    KeyRevoke {
        key_id: Uuid,
    },
    /// Offline inspection of the current SQLite identity schema; never migrates a database.
    IdentityCheck,
    /// Capture opt-in client hooks or send their private queue to the host.
    ClientHook {
        #[arg(long)]
        profile: PathBuf,
        #[command(subcommand)]
        action: HookAction,
    },
    /// Capture Run metadata for an installed client plugin.
    PluginHook {
        #[arg(long, value_enum)]
        client: HookClient,
    },
    /// Export one approved Skill copy into a Claude Code or Codex project.
    SkillExport {
        #[arg(long, value_enum)]
        client: SkillClient,
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        tenant_id: Uuid,
        #[arg(long)]
        workspace_id: Uuid,
        skill_id: Uuid,
        version: u32,
    },
    Search {
        query: String,
        #[arg(long, default_value = "keyword")]
        mode: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        allow_partial: bool,
    },
    Resolve {
        query: String,
        #[arg(long, default_value = "keyword")]
        mode: String,
        #[arg(long, default_value_t = 2000)]
        budget_tokens: usize,
        #[arg(long)]
        allow_partial: bool,
    },
    Get {
        asset_id: Uuid,
        #[arg(long)]
        version: Option<i32>,
    },
}
#[derive(Subcommand)]
enum HookAction {
    /// Read hook stdin and enqueue only allowlisted metadata. Never sends HTTP.
    Capture {
        #[arg(long, value_enum)]
        client: HookClient,
    },
    /// Send one batch, or repeat in a process independent of the client.
    Flush {
        #[arg(long)]
        watch: bool,
    },
    /// Resolve an explicit MCP binding from reliable identifiers for a turn.
    RunId {
        #[arg(long, value_enum)]
        client: HookClient,
        #[arg(long)]
        session_id: String,
        /// Codex turn_id or Claude Code prompt_id.
        #[arg(long)]
        correlation_id: String,
    },
}

type CommandFuture<'a, T> = Pin<Box<dyn Future<Output = anyhow::Result<T>> + 'a>>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing()?;
    dispatch(Cli::parse()).await
}

fn init_tracing() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "origence=info".into())
                // Upstream diagnostics may contain document internals. The
                // worker records our sanitized error code instead.
                .add_directive("pdf_oxide=off".parse()?)
                .add_directive("anyseek=off".parse()?),
        )
        .init();
    Ok(())
}

// Select and allocate one command future synchronously. Unrelated commands then
// have separate poll functions instead of sharing the CLI's async stack frame.
fn dispatch(cli: Cli) -> CommandFuture<'static, ()> {
    let Cli {
        data_dir,
        server_url,
        offline,
        command,
    } = cli;
    match command {
        Command::AnyseekSchema {
            dimensions,
            with_space,
        } => Box::pin(async move {
            let models = with_space.then(Models::from_env).transpose()?;
            let configured_dimension = models
                .as_ref()
                .and_then(|m| m.profile.as_ref().map(|_| m.embedding_dimension()));
            if with_space && dimensions.is_some() && dimensions != configured_dimension {
                anyhow::bail!("dimensions must match the configured embedding model");
            }
            let dimension = dimensions.or(configured_dimension);
            let config = match dimension {
                Some(d) => {
                    origence::storage::anyseek::AnySeekFullTextStore::vector_collection_config(d)?
                }
                None => origence::storage::anyseek::AnySeekFullTextStore::collection_config(),
            };
            let output = if let Some(models) = models {
                json!({"config":config,"embedding_space_id":models.profile})
            } else {
                config
            };
            println!("{output}");
            Ok(())
        }),
        Command::Serve {
            bind,
            mcp_public_origin,
            #[cfg(feature = "embedded-anyseek")]
            local,
        } => Box::pin(async move {
            let mcp = mcp_public_origin
                .as_deref()
                .filter(|origin| !origin.is_empty())
                .map(origence::api::McpHttpConfig::with_public_origin)
                .transpose()?
                .unwrap_or_default();
            let models = Models::from_env()?;
            #[cfg(feature = "embedded-anyseek")]
            let service = if local {
                Service::open_embedded(&data_dir, models).await?
            } else {
                Service::open_with_anyseek(&data_dir, models, AnySeekConfig::from_env()?).await?
            };
            #[cfg(not(feature = "embedded-anyseek"))]
            let service =
                Service::open_with_anyseek(&data_dir, models, AnySeekConfig::from_env()?).await?;
            match tokio::net::TcpListener::bind(bind).await {
                Ok(listener) => {
                    host::serve_http_with_mcp(service, listener, mcp, host::shutdown_signal()).await
                }
                Err(error) => {
                    service.engine.shutdown().await?;
                    Err(error.into())
                }
            }
        }),
        Command::IdentityCheck => Box::pin(async move {
            anyhow::ensure!(
                offline,
                "identity-check requires --offline and a stopped host"
            );
            SqliteStore::check_memory_identity(data_dir.join("context.db")).await?;
            println!("{}", json!({"feature":"memory-identity-v1","valid":true}));
            Ok(())
        }),
        Command::ClientHook { profile, action } => Box::pin(run_client_hook(profile, action)),
        Command::PluginHook { client } => Box::pin(async move {
            match client_hook::capture_plugin_event(client, &server_url).await {
                Ok(report) => {
                    if report.run_delivery_failed {
                        eprintln!("Origence plugin hook: Run observation could not be delivered");
                    }
                    if let Some(notice) = report.turn_sync_notice {
                        eprintln!("Origence turn sync: {notice}");
                    }
                }
                Err(_) => eprintln!("Origence plugin hook: could not read the observation"),
            }
            println!("{{}}");
            Ok(())
        }),
        command @ (Command::WorkspaceCreate { .. }
        | Command::KeyCreate { .. }
        | Command::KeyRevoke { .. })
            if offline =>
        {
            Box::pin(run_admin(data_dir, command))
        }
        Command::SkillExport { .. } if offline => {
            Box::pin(async { anyhow::bail!("skill-export requires an online Origence host") })
        }
        command if offline => Box::pin(run_offline(data_dir, command)),
        command => Box::pin(run_remote(server_url, command)),
    }
}

async fn run_client_hook(path: PathBuf, action: HookAction) -> anyhow::Result<()> {
    if let HookAction::Capture { client } = action {
        let capture = tokio::task::spawn_blocking(move || {
            let Some(profile) = HookProfile::load(&path)?.filter(|p| p.enabled(client)) else {
                return Ok(client_hook::Capture::Ignored);
            };
            let token = hook_token(&profile)?;
            profile.enqueue(
                client,
                &token,
                &std::env::current_dir()?,
                std::io::stdin().lock(),
            )
        })
        .await?;
        match capture {
            Ok(client_hook::Capture::MissingIdentity) => {
                eprintln!("Origence hook: missing reliable event or turn identity")
            }
            Ok(client_hook::Capture::Full) => {
                eprintln!("Origence hook: observation skipped because the queue is full")
            }
            Err(error) => eprintln!("Origence hook: observation skipped ({error})"),
            _ => {}
        }
        // A neutral response for Codex Stop and other command-hook schemas.
        println!("{{}}");
        return Ok(());
    }

    let mut previous = None;
    loop {
        let profile_path = path.clone();
        let Some(profile) =
            tokio::task::spawn_blocking(move || HookProfile::load(&profile_path)).await??
        else {
            return Ok(());
        };
        match &action {
            HookAction::RunId {
                client,
                session_id,
                correlation_id,
            } => {
                let run = profile
                    .run_identity(*client, session_id, correlation_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!("hook client disabled or missing reliable identity")
                    })?;
                println!("{}", serde_json::to_string(&run)?);
                return Ok(());
            }
            HookAction::Flush { watch } => {
                let token = hook_token(&profile)?;
                let report = match profile.flush(token).await {
                    Ok(report) => report,
                    Err(error) if *watch => {
                        eprintln!("Origence hook sender: local queue unavailable ({error})");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if !watch
                    || report.sent > 0
                    || report.discarded > 0
                    || previous.as_ref() != Some(&report)
                {
                    println!("{}", serde_json::to_string(&report)?);
                }
                if !watch {
                    return Ok(());
                }
                let wait = if report.sent + report.discarded > 0 {
                    Duration::from_millis(250)
                } else {
                    Duration::from_secs(5)
                };
                previous = Some(report);
                tokio::time::sleep(wait).await;
            }
            HookAction::Capture { .. } => unreachable!(),
        }
    }
}

fn hook_token(profile: &HookProfile) -> anyhow::Result<String> {
    std::env::var(profile.key_env()).map_err(|_| anyhow::anyhow!("hook credential is unavailable"))
}

async fn run_admin(data_dir: PathBuf, command: Command) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(&data_dir).await?;
    let store = SqliteStore::open(data_dir.join("context.db")).await?;
    let result = match command {
        Command::WorkspaceCreate { name, in_tenant_of } => {
            anyhow::ensure!(!name.trim().is_empty(), "name is required");
            let scope = Scope {
                tenant_id: match in_tenant_of {
                    Some(workspace) => store.workspace_scope(workspace).await?.tenant_id,
                    None => Uuid::new_v4(),
                },
                workspace_id: Uuid::new_v4(),
            };
            store
                .create_workspace(scope.tenant_id, scope.workspace_id, &name)
                .await?;
            let key = store.issue_key(scope, "admin").await?;
            json!({"tenant_id":scope.tenant_id,"workspace_id":scope.workspace_id,"key_id":key.key_id,"token":key.token})
        }
        Command::KeyCreate {
            workspace_id,
            role,
            allow_owned_workspaces,
        } => json!(
            store
                .issue_key_with_capabilities(
                    store.workspace_scope(workspace_id).await?,
                    &role,
                    KeyCapabilities {
                        allow_owned_workspaces
                    },
                )
                .await?
        ),
        Command::KeyRevoke { key_id } => {
            store.revoke_key(key_id).await?;
            json!({"key_id":key_id,"revoked":true})
        }
        _ => unreachable!(),
    };
    store.shutdown().await?;
    println!("{result}");
    Ok(())
}

async fn run_offline(data_dir: PathBuf, command: Command) -> anyhow::Result<()> {
    let token =
        std::env::var("OC_API_KEY").map_err(|_| anyhow::anyhow!("OC_API_KEY is required"))?;
    let service =
        Service::open_with_anyseek(&data_dir, Models::from_env()?, AnySeekConfig::from_env()?)
            .await?;
    let auth = service.auth(&token).await?;
    let result = offline_command(&service, &auth, command).await?;
    service.engine.shutdown().await?;
    println!("{result}");
    Ok(())
}

// Search/resolve futures also remain independent: their construction and
// serialization temporaries do not share run_offline's poll frame.
fn offline_command<'a>(
    service: &'a Service,
    auth: &'a AuthContext,
    command: Command,
) -> CommandFuture<'a, serde_json::Value> {
    match command {
        Command::Search {
            query,
            mode,
            limit,
            allow_partial,
        } => Box::pin(async move {
            Ok(service
                .search(
                    auth,
                    SearchInput {
                        components: Default::default(),
                        memory_identity: None,
                        query,
                        mode,
                        limit,
                        allow_partial,
                    },
                )
                .await?)
        }),
        Command::Resolve {
            query,
            mode,
            budget_tokens,
            allow_partial,
        } => Box::pin(async move {
            Ok(service
                .resolve(
                    auth,
                    ResolveInput {
                        components: Default::default(),
                        memory_identity: None,
                        query,
                        mode,
                        budget_tokens,
                        allow_partial,
                    },
                )
                .await?)
        }),
        Command::Get { asset_id, version } => {
            Box::pin(async move { Ok(service.get(auth, asset_id, version).await?) })
        }
        _ => unreachable!(),
    }
}

async fn run_remote(server_url: String, command: Command) -> anyhow::Result<()> {
    anyhow::ensure!(
        !matches!(command, Command::WorkspaceCreate { .. }),
        "workspace-create requires --offline and a stopped host"
    );
    let token = std::env::var("OC_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("OC_API_KEY is required"))?;
    let client = HostClient::new(&server_url, token)?;
    let (method, path, body) = match command {
        Command::SkillExport {
            client: target,
            project,
            tenant_id,
            workspace_id,
            skill_id,
            version,
        } => {
            let path = export_skill(
                &client,
                target,
                &project,
                Scope {
                    tenant_id,
                    workspace_id,
                },
                skill_id,
                version,
            )
            .await?;
            println!(
                "{}",
                json!({"skill_id":skill_id,"version":version,"path":path,"exported":true})
            );
            return Ok(());
        }
        Command::Search {
            query,
            mode,
            limit,
            allow_partial,
        } => (
            reqwest::Method::POST,
            "/v1/search".into(),
            Some(json!(SearchInput {
                components: Default::default(),
                memory_identity: None,
                query,
                mode,
                limit,
                allow_partial
            })),
        ),
        Command::Resolve {
            query,
            mode,
            budget_tokens,
            allow_partial,
        } => (
            reqwest::Method::POST,
            "/v1/resolve".into(),
            Some(json!(ResolveInput {
                components: Default::default(),
                memory_identity: None,
                query,
                mode,
                budget_tokens,
                allow_partial
            })),
        ),
        Command::Get { asset_id, version } => (
            reqwest::Method::GET,
            format!(
                "/v1/assets/{asset_id}{}",
                version.map(|v| format!("?version={v}")).unwrap_or_default()
            ),
            None,
        ),
        Command::KeyCreate {
            workspace_id,
            role,
            allow_owned_workspaces,
        } => (
            reqwest::Method::POST,
            "/admin/keys".into(),
            Some(
                json!({"workspace_id":workspace_id,"role":role,"allow_owned_workspaces":allow_owned_workspaces}),
            ),
        ),
        Command::KeyRevoke { key_id } => (
            reqwest::Method::DELETE,
            format!("/admin/keys/{key_id}"),
            None,
        ),
        _ => unreachable!(),
    };
    println!("{}", client.call(method, &path, body.as_ref(), None).await?);
    Ok(())
}

use clap::{Parser, Subcommand};
use origence::{
    client::HostClient,
    host, mcp,
    models::Models,
    service::Service,
    storage::{KeyCapabilities, Lifecycle, RelationalStore, Scope, TursoStore},
    types::{AuthContext, ResolveInput, SearchInput},
};
use serde_json::json;
use std::{future::Future, path::PathBuf, pin::Pin};
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
    /// Run the HTTP API and its single durable worker in one process.
    #[command(alias = "api")]
    Serve {
        #[arg(long, env = "OC_BIND", default_value = "127.0.0.1:8080")]
        bind: String,
        /// Enable the AML protocol routes for explicitly capable root keys.
        #[arg(long, env = "OC_ENABLE_AML", default_value_t = false)]
        enable_aml: bool,
    },
    /// Offline bootstrap: create a workspace and print its admin token once.
    WorkspaceCreate {
        name: String,
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
    /// Offline inspection of the current Turso identity schema; never migrates a database.
    IdentityCheck,
    /// Compact existing local vector files; requires a stopped host. Never prunes history.
    VectorCompact {
        #[arg(long)]
        dry_run: bool,
    },
    /// Read-only stdio MCP; forwards to the running host by default.
    Mcp,
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
                .unwrap_or_else(|_| "origence=info,turso=warn".into())
                // Upstream diagnostics may contain document internals. The
                // worker records our sanitized error code instead.
                .add_directive("pdf_oxide=off".parse()?),
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
        Command::Serve { bind, enable_aml } => Box::pin(async move {
            let service = Service::open(&data_dir, Models::from_env()?).await?;
            let listener = tokio::net::TcpListener::bind(bind).await?;
            println!(
                "{}",
                json!({"listening":listener.local_addr()?.to_string()})
            );
            host::serve_with_options(
                service,
                listener,
                origence::api::ApiOptions { enable_aml },
                host::shutdown_signal(),
            )
            .await
        }),
        Command::VectorCompact { dry_run } => Box::pin(async move {
            anyhow::ensure!(
                offline,
                "vector-compact requires --offline and a stopped host"
            );
            let report =
                origence::storage::maintenance::compact_vectors(&data_dir, dry_run).await?;
            println!("{}", serde_json::to_string(&report)?);
            Ok(())
        }),
        Command::IdentityCheck => Box::pin(async move {
            anyhow::ensure!(
                offline,
                "identity-check requires --offline and a stopped host"
            );
            TursoStore::check_memory_identity(data_dir.join("context.db")).await?;
            println!(
                "{}",
                json!({"feature":"memory-identity-v1","valid":true})
            );
            Ok(())
        }),
        command @ (Command::WorkspaceCreate { .. }
        | Command::KeyCreate { .. }
        | Command::KeyRevoke { .. })
            if offline =>
        {
            Box::pin(run_admin(data_dir, command))
        }
        command if offline => Box::pin(run_offline(data_dir, command)),
        command => Box::pin(run_remote(server_url, command)),
    }
}

async fn run_admin(data_dir: PathBuf, command: Command) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(&data_dir).await?;
    let store = TursoStore::open(data_dir.join("context.db")).await?;
    let result = match command {
        Command::WorkspaceCreate { name } => {
            anyhow::ensure!(!name.trim().is_empty(), "name is required");
            let scope = Scope {
                tenant_id: Uuid::new_v4(),
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
    let service = Service::open(&data_dir, Models::from_env()?).await?;
    let auth = service.auth(&token).await?;
    let result = offline_command(&service, &auth, token, command).await?;
    service.engine.shutdown().await?;
    if let Some(v) = result {
        println!("{v}");
    }
    Ok(())
}

// Search/resolve/MCP futures also remain independent: their construction and
// serialization temporaries do not share run_offline's poll frame.
fn offline_command<'a>(
    service: &'a Service,
    auth: &'a AuthContext,
    token: String,
    command: Command,
) -> CommandFuture<'a, Option<serde_json::Value>> {
    match command {
        Command::Mcp => Box::pin(async move {
            mcp::run(service.clone(), token).await?;
            Ok(None)
        }),
        Command::Search {
            query,
            mode,
            limit,
            allow_partial,
        } => Box::pin(async move {
            Ok(Some(
                service
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
                    .await?,
            ))
        }),
        Command::Resolve {
            query,
            mode,
            budget_tokens,
            allow_partial,
        } => Box::pin(async move {
            Ok(Some(
                service
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
                    .await?,
            ))
        }),
        Command::Get { asset_id, version } => {
            Box::pin(async move { Ok(Some(service.get(auth, asset_id, version).await?)) })
        }
        _ => unreachable!(),
    }
}

async fn run_remote(server_url: String, command: Command) -> anyhow::Result<()> {
    anyhow::ensure!(
        !matches!(command, Command::WorkspaceCreate { .. }),
        "workspace-create requires --offline and a stopped host"
    );
    let token =
        std::env::var("OC_API_KEY").map_err(|_| anyhow::anyhow!("OC_API_KEY is required"))?;
    let client = HostClient::new(&server_url, token)?;
    let (method, path, body) = match command {
        Command::Mcp => return mcp::run_remote(client).await,
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

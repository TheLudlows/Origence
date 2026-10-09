use clap::{Parser, Subcommand};
use origence::{
    client::HostClient,
    host, mcp,
    models::Models,
    parsing,
    service::Service,
    storage::{Lifecycle, RelationalStore, Scope, SqliteStore},
    types::{ResolveInput, SearchInput},
};
use serde_json::json;
use std::path::PathBuf;
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
    },
    /// Offline bootstrap: create a workspace and print its admin token once.
    WorkspaceCreate {
        name: String,
    },
    KeyCreate {
        workspace_id: Uuid,
        #[arg(long, default_value = "reader")]
        role: String,
    },
    KeyRevoke {
        key_id: Uuid,
    },
    /// Offline, explicit upgrade of an existing database to enable identity writes.
    MemoryIdentityUpgrade {
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
    #[command(hide = true)]
    ParsePdf {
        path: PathBuf,
    },
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "origence=info,sqlx=warn".into()),
        )
        .init();
    let cli = Cli::parse();
    if let Command::ParsePdf { path } = &cli.command {
        println!("{}", serde_json::to_string(&parsing::pdf_child(path)?)?);
        return Ok(());
    }
    if let Command::Serve { bind } = &cli.command {
        let service = Service::open(&cli.data_dir, Models::from_env()?).await?;
        let listener = tokio::net::TcpListener::bind(bind).await?;
        println!(
            "{}",
            json!({"listening":listener.local_addr()?.to_string()})
        );
        return host::serve(service, listener, host::shutdown_signal()).await;
    }
    if let Command::MemoryIdentityUpgrade { dry_run } = &cli.command {
        anyhow::ensure!(
            cli.offline,
            "memory-identity-upgrade requires --offline and a stopped host"
        );
        let status =
            SqliteStore::upgrade_memory_identity(cli.data_dir.join("context.db"), *dry_run).await?;
        println!(
            "{}",
            json!({"feature":"memory-identity-v1","status":status,"dry_run":dry_run})
        );
        return Ok(());
    }
    if cli.offline
        && matches!(
            cli.command,
            Command::WorkspaceCreate { .. } | Command::KeyCreate { .. } | Command::KeyRevoke { .. }
        )
    {
        tokio::fs::create_dir_all(&cli.data_dir).await?;
        let store = SqliteStore::open(cli.data_dir.join("context.db")).await?;
        let result = match cli.command {
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
            Command::KeyCreate { workspace_id, role } => json!(
                store
                    .issue_key(store.workspace_scope(workspace_id).await?, &role)
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
        return Ok(());
    }
    anyhow::ensure!(
        !matches!(cli.command, Command::WorkspaceCreate { .. }),
        "workspace-create requires --offline and a stopped host"
    );
    let token =
        std::env::var("OC_API_KEY").map_err(|_| anyhow::anyhow!("OC_API_KEY is required"))?;
    if cli.offline {
        let service = Service::open(&cli.data_dir, Models::from_env()?).await?;
        let auth = service.auth(&token).await?;
        let result = match cli.command {
            Command::Mcp => {
                mcp::run(service.clone(), token).await?;
                None
            }
            Command::Search {
                query,
                mode,
                limit,
                allow_partial,
            } => Some(
                service
                    .search(
                        &auth,
                        SearchInput {
                            memory_identity: None,
                            query,
                            mode,
                            limit,
                            allow_partial,
                        },
                    )
                    .await?,
            ),
            Command::Resolve {
                query,
                mode,
                budget_tokens,
                allow_partial,
            } => Some(
                service
                    .resolve(
                        &auth,
                        ResolveInput {
                            memory_identity: None,
                            query,
                            mode,
                            budget_tokens,
                            allow_partial,
                        },
                    )
                    .await?,
            ),
            Command::Get { asset_id, version } => {
                Some(service.get(&auth, asset_id, version).await?)
            }
            _ => unreachable!(),
        };
        service.engine.shutdown().await?;
        if let Some(v) = result {
            println!("{v}");
        }
        return Ok(());
    }
    let client = HostClient::new(&cli.server_url, token)?;
    let (method, path, body) = match cli.command {
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
        Command::KeyCreate { workspace_id, role } => (
            reqwest::Method::POST,
            "/admin/keys".into(),
            Some(json!({"workspace_id":workspace_id,"role":role})),
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

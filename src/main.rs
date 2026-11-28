use clap::{Parser, Subcommand};
use origence::{
    client_hook::{self, Client as HookClient},
    host,
    models::Models,
    service::Service,
    storage::anyseek::AnySeekConfig,
};
use std::{future::Future, path::PathBuf, pin::Pin};

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
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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
    /// Capture Run metadata for an installed client plugin.
    PluginHook {
        #[arg(long, value_enum)]
        client: HookClient,
    },
}

type CommandFuture = Pin<Box<dyn Future<Output = anyhow::Result<()>>>>;

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
fn dispatch(cli: Cli) -> CommandFuture {
    let Cli {
        data_dir,
        server_url,
        command,
    } = cli;
    match command {
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
            if let Err(error) = host::initialize_deployment_admin(&service).await {
                service.engine.shutdown().await?;
                return Err(error);
            }
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
    }
}

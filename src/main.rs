use clap::{Parser, Subcommand};
use origence::{host, models::Models, service::Service, storage::anyseek::AnySeekConfig};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[arg(long, env = "OC_DATA_DIR", default_value = ".data", global = true)]
    data_dir: PathBuf,
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing()?;
    Box::pin(dispatch(Cli::parse())).await
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

async fn dispatch(cli: Cli) -> anyhow::Result<()> {
    let Cli { data_dir, command } = cli;
    let Command::Serve {
        bind,
        mcp_public_origin,
        #[cfg(feature = "embedded-anyseek")]
        local,
    } = command;
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
    let service = Service::open_with_anyseek(&data_dir, models, AnySeekConfig::from_env()?).await?;
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
}

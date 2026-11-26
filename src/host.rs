use crate::{api, service::Service, worker};
use std::future::Future;
use tokio::{net::TcpListener, sync::watch};

/// Report the CLI listener address, then run the HTTP server and worker.
pub async fn serve_http(
    service: Service,
    listener: TcpListener,
    shutdown: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    serve_http_with_mcp(service, listener, api::McpHttpConfig::default(), shutdown).await
}

pub async fn serve_http_with_mcp(
    service: Service,
    listener: TcpListener,
    mcp: api::McpHttpConfig,
    shutdown: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::json!({"listening":listener.local_addr()?.to_string()})
    );
    serve_with_mcp(service, listener, mcp, shutdown).await
}

pub async fn serve(
    service: Service,
    listener: TcpListener,
    shutdown: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    serve_with_mcp(service, listener, api::McpHttpConfig::default(), shutdown).await
}

async fn serve_with_mcp(
    service: Service,
    listener: TcpListener,
    mcp: api::McpHttpConfig,
    shutdown: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    if let Err(error) = service.engine.check().await {
        let _ = service.engine.shutdown().await;
        return Err(error.into());
    }
    let (stop, rx) = watch::channel(false);
    let api_stop = rx.clone();
    let mut worker = tokio::spawn(worker::run(service.clone(), rx));
    let api_service = service.clone();
    let mut server = tokio::spawn(async move {
        axum::serve(listener, api::router_with_mcp(api_service, mcp))
            .with_graceful_shutdown(async move {
                let mut rx = api_stop;
                let _ = rx.wait_for(|v| *v).await;
            })
            .await
    });
    let result = async { tokio::select! {
        _=shutdown=>{let _=stop.send(true);let (server, worker) = tokio::join!(server, worker);server??;worker??;Ok(())},
        result=&mut worker=>{let _=stop.send(true);server.await??;result??;anyhow::bail!("worker stopped unexpectedly")},
        result=&mut server=>{let _=stop.send(true);worker.await??;result??;Ok(())}
    }}.await;
    service.engine.shutdown().await?;
    result
}
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {_ = tokio::signal::ctrl_c()=>{}, _ = term.recv()=>{}}
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

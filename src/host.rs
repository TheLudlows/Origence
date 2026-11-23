use crate::{api, models::Models, service::Service, worker};
use std::{future::Future, path::Path, pin::pin};
use tokio::{net::TcpListener, sync::watch};

mod local_anyseek;
pub use local_anyseek::LocalAnySeek;

/// Run the host with an AnySeek process owned by this invocation.
pub async fn serve_local(
    data_dir: &Path,
    models: Models,
    listener: TcpListener,
    anyseek: LocalAnySeek,
) -> anyhow::Result<()> {
    let mut shutdown = pin!(shutdown_signal());
    let (mut child, config) = tokio::select! {
        biased;
        _ = &mut shutdown => return Ok(()),
        started = anyseek.start(data_dir) => started?,
    };
    let result = async {
        let service = tokio::select! {
            biased;
            _ = &mut shutdown => return Ok(()),
            status = child.wait() => anyhow::bail!("local AnySeek exited during startup ({})", status?),
            service = async {
                local_anyseek::wait_ready(config.clone()).await?;
                Service::open_with_anyseek(data_dir, models, config).await.map_err(anyhow::Error::from)
            } => service?,
        };
        let mut child_exit = None;
        let result = serve_http(service, listener, async {
            tokio::select! {
                biased;
                _ = &mut shutdown => {},
                status = child.wait() => child_exit = Some(status),
            }
        })
        .await;
        if let Some(status) = child_exit {
            anyhow::bail!("local AnySeek exited while Origence was running ({})", status?);
        }
        result
    }
    .await;
    let stopped = local_anyseek::stop(&mut child).await;
    result.and(stopped)
}

/// Report the CLI listener address, then run the HTTP server and worker.
pub async fn serve_http(
    service: Service,
    listener: TcpListener,
    shutdown: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::json!({"listening":listener.local_addr()?.to_string()})
    );
    serve(service, listener, shutdown).await
}

pub async fn serve(
    service: Service,
    listener: TcpListener,
    shutdown: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    service.engine.check().await?;
    let (stop, rx) = watch::channel(false);
    let api_stop = rx.clone();
    let mut worker = tokio::spawn(worker::run(service.clone(), rx));
    let api_service = service.clone();
    let mut server = tokio::spawn(async move {
        axum::serve(listener, api::router(api_service))
            .with_graceful_shutdown(async move {
                let mut rx = api_stop;
                let _ = rx.wait_for(|v| *v).await;
            })
            .await
    });
    let result = async { tokio::select! {
        _=shutdown=>{let _=stop.send(true);server.await??;worker.await??;Ok(())},
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

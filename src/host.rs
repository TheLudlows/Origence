use crate::{api, service::Service, worker};
use std::future::Future;
use tokio::{net::TcpListener, sync::watch};

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

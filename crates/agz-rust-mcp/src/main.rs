#![forbid(unsafe_code)]

use std::{error::Error, future::Future, sync::Arc, time::Duration};

use agz_rust_mcp::{
    AppState, CliOptions, Config, RustMcpServer,
    config::{CliCommand, Transport},
};
use clap::{Parser, error::ErrorKind};
use rmcp::{ServiceExt, transport::stdio};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

type MainError = Box<dyn Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), MainError> {
    match agz_rust_mcp::process::root_bound::maybe_run_root_guard() {
        Ok(Some(())) => return Ok(()),
        Ok(None) => {}
        Err(error) => return Err(error.into()),
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .try_init()
        .ok();

    let cli = match CliOptions::try_parse_from(std::env::args_os()) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            error.print()?;
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    if let Some(CliCommand::Skills { action }) = &cli.command {
        agz_rust_mcp::skills::run(action)?;
        return Ok(());
    }
    let config = Config::load_cli(&cli)?;
    let transport = config.server.transport;
    let http_bind = config.server.http_bind;
    let server = RustMcpServer::new(config)?;
    let state = server.state().clone();

    let transport_result = match transport {
        Transport::Stdio => {
            let service = Box::pin(server.serve(stdio())).await?;
            let cancellation = service.cancellation_token();
            wait_for_shutdown(
                async move { service.waiting().await.map(|_| ()).map_err(Into::into) },
                &state,
                move || cancellation.cancel(),
            )
            .await
        }
        Transport::Http => {
            let cancellation = CancellationToken::new();
            let http_cancellation = cancellation.clone();
            let http_state = Arc::clone(&state);
            wait_for_shutdown(
                async move {
                    let listener = tokio::net::TcpListener::bind(http_bind).await?;
                    agz_rust_mcp::transport::serve_http(listener, http_state, http_cancellation)
                        .await?;
                    Ok(())
                },
                &state,
                move || cancellation.cancel(),
            )
            .await
        }
    };

    let shutdown_result = tokio::time::timeout(Duration::from_secs(5), state.shutdown_async())
        .await
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "domain cleanup did not stop within the shutdown deadline",
            )
        })?;
    transport_result?;
    shutdown_result?;
    Ok(())
}

async fn wait_for_shutdown(
    waiting: impl Future<Output = Result<(), MainError>>,
    state: &AppState,
    cancel_transport: impl FnOnce(),
) -> Result<(), MainError> {
    tokio::pin!(waiting);

    tokio::select! {
        result = &mut waiting => result,
        signal = shutdown_signal() => {
            state.begin_shutdown();
            cancel_transport();
            let stopped = tokio::time::timeout(Duration::from_secs(5), &mut waiting).await;
            signal?;
            stopped.unwrap_or_else(|_| Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "MCP transport did not stop within the shutdown deadline",
            ).into()))
        }
    }
}

#[cfg(unix)]
async fn shutdown_signal() -> std::io::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = terminate.recv() => Ok(()),
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() -> std::io::Result<()> {
    tokio::signal::ctrl_c().await
}

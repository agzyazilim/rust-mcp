//! Native Streamable HTTP transport for local MCP clients.
//!
//! The application state is owned by one server process, while each legacy
//! session (or modern stateless request) gets its own client-root coordinator.
//! This preserves changesets and task IDs without sharing a client's authority.

use std::{future::IntoFuture, io, sync::Arc, time::Duration};

use axum::{
    Router,
    extract::Request,
    http::StatusCode,
    middleware::{self, Next},
    response::IntoResponse,
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService,
    session::{SessionManager, local::LocalSessionManager},
};
use tokio::{net::TcpListener, sync::Semaphore};
use tokio_util::sync::CancellationToken;

use crate::{AppState, RustMcpServer, config::Transport};

const MAX_HTTP_SERVICES: usize = 64;
const MAX_HTTP_BODY_BYTES: usize = 1024 * 1024;

/// Serve MCP at `/mcp` until cancellation. The caller owns application cleanup
/// and must call `AppState::shutdown_async` after the transport stops.
///
/// Only a loopback listener is accepted. Remote access belongs behind an
/// authenticated HTTPS proxy; this transport does not expose a public service.
/// HTTP requests with any browser Origin are rejected. Native MCP clients omit
/// Origin, and must use the listener's authority or `localhost` with its port.
///
/// # Errors
/// Returns an error for a non-loopback listener, non-HTTP application state,
/// a failed HTTP server, or incomplete legacy session cleanup.
pub async fn serve_http(
    listener: TcpListener,
    state: Arc<AppState>,
    cancellation: CancellationToken,
) -> io::Result<()> {
    let address = listener.local_addr()?;
    if !address.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HTTP requires a loopback listener",
        ));
    }
    if state.config().server.transport != Transport::Http {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HTTP application state requires server.transport=http",
        ));
    }

    let mut transport_config = StreamableHttpServerConfig::default()
        .with_allowed_hosts([address.to_string(), format!("localhost:{}", address.port())])
        .enforce_origin_validation()
        .with_json_response(true);
    transport_config.cancellation_token = cancellation.clone();
    transport_config.max_request_body_bytes = MAX_HTTP_BODY_BYTES;

    let capacity = Arc::new(Semaphore::new(MAX_HTTP_SERVICES));
    let factory_cancellation = cancellation.clone();
    let sessions = Arc::new(LocalSessionManager::default());
    let service = StreamableHttpService::new(
        move || {
            if factory_cancellation.is_cancelled() || state.is_shutting_down() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "HTTP server is shutting down",
                ));
            }
            let permit = Arc::clone(&capacity).try_acquire_owned().map_err(|_| {
                io::Error::new(io::ErrorKind::WouldBlock, "HTTP session capacity reached")
            })?;
            Ok(RustMcpServer::from_state(Arc::clone(&state)).with_connection_permit(permit))
        },
        Arc::clone(&sessions),
        transport_config,
    );
    let request_cancellation = cancellation.clone();
    let router = Router::new().nest_service("/mcp", service).layer(middleware::from_fn(
        move |request: Request, next: Next| {
            let cancellation = request_cancellation.clone();
            async move {
                tokio::select! {
                    response = next.run(request) => response,
                    () = cancellation.cancelled() => StatusCode::SERVICE_UNAVAILABLE.into_response(),
                }
            }
        },
    ));
    tracing::info!(%address, "Streamable HTTP MCP listening at /mcp");
    let result = {
        let serving = axum::serve(listener, router)
            .with_graceful_shutdown(cancellation.clone().cancelled_owned())
            .into_future();
        tokio::pin!(serving);
        tokio::select! {
            result = &mut serving => result,
            () = cancellation.cancelled() => {
                tokio::time::timeout(Duration::from_secs(2), &mut serving).await
                    .unwrap_or_else(|_| Err(io::Error::new(io::ErrorKind::TimedOut, "HTTP drain timed out")))
            }
        }
    };
    cancellation.cancel();
    // The SDK closes HTTP response streams on cancellation, but legacy session
    // workers otherwise retain their handler/state until the idle timeout.
    // HTTP has stopped accepting requests here, so no new session can race this
    // final sweep. Explicitly close them for embedding/restart as well as CLI exit.
    let ids: Vec<_> = sessions.sessions.read().await.keys().cloned().collect();
    tokio::time::timeout(Duration::from_secs(2), async {
        for id in ids {
            sessions
                .close_session(&id)
                .await
                .map_err(io::Error::other)?;
        }
        Ok::<_, io::Error>(())
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "HTTP session cleanup timed out"))??;
    result
}

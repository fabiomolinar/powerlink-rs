//! Implements the core web server and WebSocket logic using axum.

use crate::model::DiagnosticSnapshot;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use log::{error, info, trace};
use std::net::SocketAddr;
use tokio::sync::broadcast;

/// The shared application state, containing the broadcast channel
/// to send snapshots to all connected WebSocket clients.
#[derive(Clone)]
pub(super) struct AppState {
    /// Sender for broadcasting diagnostic snapshots to all connected clients.
    pub(super) snapshot_tx: broadcast::Sender<DiagnosticSnapshot>,
}

/// The main entry point for starting the web server.
///
/// This function binds to the given address and sets up all routes.
pub(super) async fn start_web_server(
    addr: SocketAddr,
    snapshot_tx: broadcast::Sender<DiagnosticSnapshot>,
) {
    let app_state = AppState { snapshot_tx };

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/ws", get(websocket_handler))
        .with_state(app_state);

    info!("Web monitor listening on http://{}", addr);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            error!("Failed to bind web server to {}: {}", addr, e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        error!("Web server encountered an error: {}", e);
    }
}

/// Handles the root HTTP GET request, serving the embedded monitor HTML.
async fn root_handler() -> impl IntoResponse {
    // Embed the HTML file directly into the binary
    Html(include_str!("web/monitor.html"))
}

/// Handles HTTP GET requests to `/ws`, upgrading them to a WebSocket connection.
async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// The main task for an individual WebSocket connection.
///
/// This task subscribes to the broadcast channel and sends any received
/// `DiagnosticSnapshot` as a JSON message to the client.
async fn handle_socket(mut socket: WebSocket, state: AppState) {
    info!("New WebSocket client connected.");

    // Subscribe to the broadcast channel to receive new snapshots.
    let mut snapshot_rx = state.snapshot_tx.subscribe();

    loop {
        tokio::select! {
            // Receive a new snapshot from the broadcast channel
            Ok(snapshot) = snapshot_rx.recv() => {
                trace!("Received new snapshot, sending to WebSocket client.");
                match serde_json::to_string(&snapshot) {
                    Ok(json_payload) => {
                        if socket.send(Message::Text(json_payload.into())).await.is_err() {
                            // Client disconnected
                            info!("WebSocket client disconnected (send error).");
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Failed to serialize snapshot to JSON: {}", e);
                    }
                }
            }
            // Receive a message from the client (e.g., ping/pong or close)
            Some(Ok(msg)) = socket.recv() => {
                if let Message::Close(_) = msg {
                    info!("WebSocket client disconnected (received close message).");
                    break;
                }
                // We can ignore other messages (like ping) as axum handles pongs.
            }
            // Client disconnected without a close message
            else => {
                info!("WebSocket client disconnected (channel closed).");
                break;
            }
        }
    }
}
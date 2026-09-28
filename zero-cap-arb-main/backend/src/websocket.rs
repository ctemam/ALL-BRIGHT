use crate::api::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::{info, warn};

/// Handle WebSocket upgrade for real-time radar updates
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state.scanner.clone()))
}

async fn handle_socket(socket: WebSocket, scanner: Arc<RadarScanner>) {
    let (mut sender, mut receiver) = socket.split();

    // Send initial connection confirmation
    let init_msg = json!({
        "type": "connected",
        "message": "Zero-Cap Arbitrage Radar connected",
        "version": "0.1.0"
    });

    if sender
        .send(Message::Text(init_msg.to_string()))
        .await
        .is_err()
    {
        return;
    }

    // Spawn a task to send periodic heartbeats
    let mut heartbeat_interval = time::interval(Duration::from_secs(15));
    let mut scan_requests = Vec::new();

    loop {
        tokio::select! {
            // Handle incoming messages from client
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<serde_json::Value>(&text) {
                            Ok(payload) => {
                                if let Some(msg_type) = payload["type"].as_str() {
                                    match msg_type {
                                        "scan" => {
                                            if let Some(token) = payload["token"].as_str() {
                                                let token_address = payload["token_address"].as_str();
                                                let scanner = scanner.clone();
                                                let mut sender_clone = sender.clone();

                                                tokio::spawn(async move {
                                                    match scanner.scan_token(token, token_address).await {
                                                        Ok(result) => {
                                                            let resp = json!({
                                                                "type": "scan_result",
                                                                "data": result
                                                            });
                                                            let _ = sender_clone
                                                                .send(Message::Text(resp.to_string()))
                                                                .await;
                                                        }
                                                        Err(e) => {
                                                            let err = json!({
                                                                "type": "error",
                                                                "message": format!("Scan failed: {}", e)
                                                            });
                                                            let _ = sender_clone
                                                                .send(Message::Text(err.to_string()))
                                                                .await;
                                                        }
                                                    }
                                                });
                                            }
                                        }
                                        "subscribe" => {
                                            if let Some(token) = payload["token"].as_str() {
                                                scan_requests.push(token.to_string());
                                                info!("Client subscribed to: {}", token);

                                                let ack = json!({
                                                    "type": "subscribed",
                                                    "token": token
                                                });
                                                let _ = sender
                                                    .send(Message::Text(ack.to_string()))
                                                    .await;
                                            }
                                        }
                                        "unsubscribe" => {
                                            if let Some(token) = payload["token"].as_str() {
                                                scan_requests.retain(|t| t != token);
                                                info!("Client unsubscribed from: {}", token);
                                            }
                                        }
                                        "ping" => {
                                            let pong = json!({"type": "pong"});
                                            let _ = sender
                                                .send(Message::Text(pong.to_string()))
                                                .await;
                                        }
                                        _ => {
                                            let err = json!({
                                                "type": "error",
                                                "message": format!("Unknown message type: {}", msg_type)
                                            });
                                            let _ = sender
                                                .send(Message::Text(err.to_string()))
                                                .await;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Invalid WebSocket message: {}", e);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        warn!("WebSocket error: {}", e);
                        break;
                    }
                }
            }

            // Periodic heartbeat
            _ = heartbeat_interval.tick() => {
                let heartbeat = json!({"type": "heartbeat"});
                if sender.send(Message::Text(heartbeat.to_string())).await.is_err() {
                    break;
                }
            }
        }
    }

    info!("WebSocket connection closed");
}

use std::sync::Arc;
use std::time::Duration;

use axum::{extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    State,
}, response::{IntoResponse}};
use futures::{
    sink::SinkExt, stream::StreamExt,
    stream::SplitSink
};
use tokio::{
    time::timeout,
    sync::Mutex
};

use super::state::ServerState;


/// After the websocket_timeout, we send a ping to the client.
/// If we still don't receive a response within this many seconds, we close the connection.
const LAST_RESORT_PING_TIMEOUT: Duration = Duration::from_secs(5);


pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<&'static ServerState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket(socket, state))
}

/// This function deals with a single websocket connection with a connected user
pub async fn websocket(stream: WebSocket, state: &'static ServerState) {
    let _track_conn = state.stats.web_socket.track_new_connection();

    // By splitting, we can send and receive at the same time.
    let (sender, mut receiver) = stream.split();
    // We want to send from multiple tasks, so we need to wrap it in a Mutex.
    let sender = Arc::new(Mutex::new(sender));
    let broadcast_sender = sender.clone();

    // A task that will receive broadcast messages and send them to the client.
    let mut broadcast_task = tokio::spawn(async move {
        let mut rx = state.broadcast_tx.subscribe();
        while let Ok(msg) = rx.recv().await {
            if !send_msg(&broadcast_sender, state, Message::Text(msg)).await {
                break;
            }
        }
    });

    // A task to receive and process messages from the client in a loop.
    let mut recv_task = tokio::spawn(async move {
        let mut username = String::new();
        let mut timeout_duration = state.web_socket_timeout;
        loop {
            match timeout(timeout_duration, receiver.next()).await {
                Ok(Some(msg)) => {
                    match msg {
                        Ok(Message::Close(_)) => {
                            state.stats.web_socket.track_client_close();
                            break;
                        },
                        Ok(Message::Text(text)) => {
                            // If we don't have a username yet, this message is the username
                            if username.is_empty() {
                                if check_username(state, &text) {
                                    username = text;
                                    let msg = format!("{} joined.", &username);
                                    tracing::debug!("{}", msg);
                                    let _ = state.broadcast_tx.send(msg);
                                } else {
                                    // If username is taken, close connection.
                                    send_msg(&sender, state, Message::Text(format!("Username {} is taken", &text))).await;
                                    send_msg(&sender, state, Message::Close(None)).await;
                                    break;
                                }
                            } else if text == "PING" {
                                // This is a client-side ping, send a response.
                                tracing::debug!("Received PING from {}", &username);
                                send_msg(&sender, state, Message::Text("PONG".to_string())).await;
                            } else {
                                // Add username before message and broadcast it to the chat.
                                let _ = state.broadcast_tx.send(format!("{}: {}", username, text));
                            }
                        },
                        Ok(Message::Pong(_)) => {
                            // We got a response to our ping, so reset the timeout.
                            tracing::debug!("Received Pong from {}", &username);
                            timeout_duration = state.web_socket_timeout;
                        },
                        Err(_) => {
                            state.stats.web_socket.track_recv_error();
                            break;
                        },
                        _ => {}
                    }
                },
                // receiver.next returned None, we're done.
                Ok(None) => break,
                // Timeout
                Err(_) => {
                    if timeout_duration == LAST_RESORT_PING_TIMEOUT {
                        // If we still got a timeout after sending a ping, close the connection.
                        tracing::debug!("Closing connection for {} due to timeout", &username);
                        state.stats.web_socket.track_timeout();
                        break;
                    }

                    tracing::debug!("Sending ping to {} to see if connection is still valid", &username);
                    // If we get a timeout, send a ping to the client.
                    // If we don't hear back in 5 seconds, we'll close the connection.
                    if !send_msg(&sender, state, Message::Ping(Vec::new())).await {
                        break;
                    }
                    timeout_duration = LAST_RESORT_PING_TIMEOUT;
                },
            }
        }

        if !username.is_empty() {
            // Send "user left" message (similar to "joined" above).
            let msg = format!("{} left.", &username);
            tracing::debug!("{}", msg);
            let _ = state.broadcast_tx.send(msg);

            // Remove username from map so new clients can take it again.
            state.user_set.lock().unwrap().remove(&username);
        }
    });

    // If any one of the tasks run to completion, we abort the other.
    tokio::select! {
        _ = (&mut broadcast_task) => recv_task.abort(),
        _ = (&mut recv_task) => broadcast_task.abort(),
    }
}

// A convenience function to send a message to the client with a timeout, and track any errors.
// Returns true if the message was sent successfully.
async fn send_msg(sender: &Mutex<SplitSink<WebSocket, Message>>, state: &'static ServerState, msg: Message) -> bool {
    match timeout(state.web_socket_timeout, sender.lock().await.send(msg)).await {
        // Message was sent successfully
        Ok(Ok(_)) => {
            true
        },
        // Error sending message
        Ok(Err(_)) => {
            state.stats.web_socket.track_send_error();
            false
        },
        // Timeout sending message.
        // The socket send buffer is full and didn't drain before the timeout.
        // The client connection is too slow to keep up with the server.
        Err(_) => {
            state.stats.web_socket.track_timeout();
            false
        }
    }
}

fn check_username(state: &ServerState, name: &str) -> bool {
    let mut user_set = state.user_set.lock().unwrap();
    if !user_set.contains(name) {
        user_set.insert(name.to_owned());
        true
    } else {
        false
    }
}
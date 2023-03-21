// Based on the MIT-licensed axum WebSocket example at https://github.com/tokio-rs/axum/blob/main/examples/chat/src/main.rs
mod server;

use std::net::SocketAddr;
use std::str::FromStr;

use axum::{
    response::{Html},
    routing::get,
    Router,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use server::{
    state::{ServerState},
    ws::{websocket_handler},
    status::{status},
};


#[tokio::main(flavor = "multi_thread")]
async fn main() {
    // Load .env file if it exists
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                // rename this to the crate name in Cargo.toml with - replaced by _
                .unwrap_or_else(|_| "rust_websocket=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Set up application state for use with with_state().
    // We don't need Arc here, this lives as long as the process does
    let state = ServerState::configure();

    let app = Router::new()
        .route("/", get(index))
        .route("/status", get(status))
        .route("/websocket", get(websocket_handler))
        .with_state(state);

    let addr = SocketAddr::from_str(&format!("{}:{}", state.listen_host, state.listen_port)).unwrap();
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

// The client-side chat app for testing
async fn index() -> Html<&'static str> {
    Html(std::include_str!("../chat.html"))
}

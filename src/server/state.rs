use std::{
    collections::HashSet,
    sync::{Mutex},
    time::Duration,
};
use std::sync::atomic::{
    AtomicPtr,
    Ordering::{Acquire, AcqRel},
};
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use tokio::sync::broadcast;
use tracing::log;

use super::stats::Stats;

static STATE: AtomicPtr<ServerState> = AtomicPtr::new(std::ptr::null_mut());

/// Get the current global ServerState instance. set_state() must be called first, or this will panic.
pub fn get_state() -> &'static ServerState {
    let ptr = STATE.load(Acquire);
    if ptr.is_null() {
        panic!("ServerState not initialized");
    }
    // Safety: if the pointer is not set to a valid ServerState, this panics above.
    unsafe { &*ptr }
}

/// Set the global ServerState instance
pub fn set_state(new_state: ServerState) {
    // Move new_state to the heap and leak it so we can store it in AtomicPtr
    let state = Box::leak(Box::new(new_state));
    let prev = STATE.swap(state as *mut _, AcqRel);
    // Safety: restore the Box from the previous pointer, if it's not null.
    if !prev.is_null() {
        unsafe {
            // Restore the original boxed value and let it be dropped here.
            _ = Box::from_raw(prev);
        }
    }
}

// Global state shared between all connected clients as a shared reference.
// Any mutable data must be behind a Mutex or similar.
pub struct ServerState {
    // We require unique usernames. This tracks which usernames have been taken.
    pub user_set: Mutex<HashSet<String>>,
    // Channel used to send messages to all connected clients.
    pub broadcast_tx: broadcast::Sender<String>,
    /// The host the HTTP server listens on (default 127.0.0.1)
    pub listen_host: String,
    /// The port the HTTP server listens on (default 4000)
    pub listen_port: u16,
    /// The port the HTTPS server listens on (default 0 - disabled)
    pub listen_port_tls: u16,
    /// The database connection pool
    pub db: DatabaseConnection,
    // If no message is received in this many seconds, we send a ping.
    // If still no response after an additional 5 seconds, the WebSocket connection is closed.
    pub web_socket_timeout: Duration,
    // Realtime statistics about the server.
    pub stats: Stats,
    // If set, this is the secret password required to access the status endpoint.
    pub status_key: String,
}

impl ServerState {
    /// Create a new ServerState and configure it from the environment.
    /// See .env file or this code for details.
    pub async fn configure() -> Self {
        let host = std::env::var("HOST").unwrap_or("127.0.0.1".to_string());
        let port = std::env::var("PORT").ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4000);
        let tls_port = std::env::var("TLS_PORT").ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let web_socket_timeout = Duration::from_secs(std::env::var("WEBSOCKET_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(25));

        let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
        let max_connections = std::env::var("DATABASE_MAX_CONNECTIONS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(100);
        let min_connections = std::env::var("DATABASE_MIN_CONNECTIONS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);
        let idle_timeout = Duration::from_secs(std::env::var("DATABASE_IDLE_TIMEOUT_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3600));
        let max_lifetime = Duration::from_secs(std::env::var("DATABASE_MAX_LIFETIME_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5));
        let connect_timeout = Duration::from_secs(std::env::var("DATABASE_CONNECT_TIMEOUT_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(web_socket_timeout.as_secs()/2));
        let log_queries = matches!(
            std::env::var("DATABASE_LOG_QUERIES").unwrap_or_default().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes" | "on");
        let status_key = std::env::var("STATUS_KEY").unwrap_or_default();

        let mut opts = ConnectOptions::new(db_url);
        opts
            .max_connections(max_connections)
            .min_connections(min_connections)
            .connect_timeout(connect_timeout)
            .acquire_timeout(connect_timeout)
            .idle_timeout(idle_timeout)
            .max_lifetime(max_lifetime)
            .sqlx_logging(log_queries)
            .sqlx_logging_level(log::LevelFilter::Info);

        let db = Database::connect(opts).await.unwrap();
        let (tx, _) = broadcast::channel(100);
        Self {
            user_set: Mutex::new(HashSet::new()),
            broadcast_tx: tx,
            listen_host: host,
            listen_port: port,
            listen_port_tls: tls_port,
            db,
            web_socket_timeout,
            stats: Stats::new(),
            status_key,
        }
    }
}
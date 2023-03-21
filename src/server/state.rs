use std::{
    collections::HashSet,
    sync::{Mutex},
    time::Duration,
};
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use tokio::sync::{broadcast, OnceCell};
use tracing::log;

use super::stats::Stats;

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
    db: OnceCell<DatabaseConnection>,
    // If no message is received in this many seconds, we send a ping.
    // If still no response after an additional 5 seconds, the WebSocket connection is closed.
    pub web_socket_timeout: Duration,
    // Realtime statistics about the server.
    pub stats: Stats,
    // If set, this is the secret password required to access the status endpoint.
    pub status_key: String,
    db_options: ConnectOptions,
}

impl ServerState {
    /// Create a new ServerState on the heap and leak it.
    /// It's a singleton that lives as long as the process does.
    /// This seems bad because drop functions won't be called, but the process only exits
    /// if it's killed, if there's a panic and it aborts, the system crashes, etc.
    /// In all those cases, drop functions won't be called anyway.
    pub fn configure() -> &'static Self {
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

        let mut db_options = ConnectOptions::new(db_url);
        db_options
            .max_connections(max_connections)
            .min_connections(min_connections)
            .connect_timeout(connect_timeout)
            .acquire_timeout(connect_timeout)
            .idle_timeout(idle_timeout)
            .max_lifetime(max_lifetime)
            .sqlx_logging(log_queries)
            .sqlx_logging_level(log::LevelFilter::Info);

        let (tx, _) = broadcast::channel(100);
        &*Box::leak(Box::new(Self {
            user_set: Mutex::new(HashSet::new()),
            broadcast_tx: tx,
            listen_host: host,
            listen_port: port,
            listen_port_tls: tls_port,
            db: OnceCell::const_new(),
            web_socket_timeout,
            stats: Stats::new(),
            status_key: std::env::var("STATUS_KEY").unwrap_or_default(),
            db_options,
        }))
    }

    pub async fn db(&self) -> &DatabaseConnection {
        self.db.get_or_init(|| async {
            Database::connect(self.db_options.clone()).await.unwrap()
        }).await
    }
}
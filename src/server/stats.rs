use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Duration;

use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use tokio::time::Instant;

#[derive(Debug)]
pub struct WebSocketStats {
    /// The total number of connections established since the server started
    connections_established: AtomicU64,
    /// The total number of connections closed since the server started
    connections_closed: AtomicU64,
    /// The number of connections closed by the client, where we received a close message
    connections_closed_by_client: AtomicU64,
    /// The number of connections aborted because there was an error in recv
    connections_recv_error: AtomicU64,
    /// The number of connections aborted because there was an error in send
    connections_send_error: AtomicU64,
    /// The number of connections aborted because there was no message received before the timeout elapsed
    connections_timed_out: AtomicU64,
    /// The total number of milliseconds spent in all connections since the server started
    total_duration_ms: AtomicU64,
}

pub struct ConnectionTracker<'a> {
    stats: &'a WebSocketStats,
    start_time: Instant,
}

impl<'a> ConnectionTracker<'a> {
    pub fn new(stats: &'a WebSocketStats) -> Self {
        Self {
            stats,
            start_time: Instant::now(),
        }
    }
}

impl<'a> Drop for ConnectionTracker<'a> {
    fn drop(&mut self) {
        let end_time = Instant::now();
        let duration = end_time - self.start_time;
        self.stats.track_close_connection(duration);
    }
}

impl WebSocketStats {
    pub fn new() -> Self {
        Self {
            connections_established: AtomicU64::new(0),
            connections_closed: AtomicU64::new(0),
            connections_closed_by_client: AtomicU64::new(0),
            connections_recv_error: AtomicU64::new(0),
            connections_send_error: AtomicU64::new(0),
            connections_timed_out: AtomicU64::new(0),
            total_duration_ms: AtomicU64::new(0),
        }
    }

    /// Increments the number of connections established and returns a ConnectionTracker
    /// that will increment the number of connections closed when it is dropped
    pub fn track_new_connection(&self) -> ConnectionTracker {
        self.connections_established.fetch_add(1, Relaxed);
        ConnectionTracker::new(self)
    }

    fn track_close_connection(&self, duration: Duration) {
        self.connections_closed.fetch_add(1, Relaxed);
        self.total_duration_ms.fetch_add(duration.as_millis() as u64, Relaxed);
    }

    pub fn track_client_close(&self) {
        self.connections_closed_by_client.fetch_add(1, Relaxed);
    }

    pub fn track_recv_error(&self) {
        self.connections_recv_error.fetch_add(1, Relaxed);
    }

    pub fn track_send_error(&self) {
        self.connections_send_error.fetch_add(1, Relaxed);
    }

    pub fn track_timeout(&self) {
        self.connections_timed_out.fetch_add(1, Relaxed);
    }

    /// Returns the average duration in seconds of all connections that have been closed
    pub fn average_duration_seconds(&self) -> f64 {
        let total_duration_ms = self.total_duration_ms.load(Relaxed);
        let connections_closed = self.connections_closed.load(Relaxed);
        if connections_closed == 0 {
            0.0
        } else {
            (total_duration_ms/connections_closed) as f64 / 1000.0
        }
    }
}

impl Serialize for WebSocketStats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let established = &self.connections_established.load(Relaxed);
        let closed = &self.connections_closed.load(Relaxed);
        let active = established - closed;
        let mut state = serializer.serialize_struct("WebSocketStats", 8)?;
        state.serialize_field("connections_active", &active)?;
        state.serialize_field("connections_established", &established)?;
        state.serialize_field("connections_closed", &closed)?;
        state.serialize_field("connections_closed_by_client", &self.connections_closed_by_client.load(Relaxed))?;
        state.serialize_field("connections_recv_error", &self.connections_recv_error.load(Relaxed))?;
        state.serialize_field("connections_send_error", &self.connections_send_error.load(Relaxed))?;
        state.serialize_field("connections_timed_out", &self.connections_timed_out.load(Relaxed))?;
        state.serialize_field("average_duration_seconds", &self.average_duration_seconds())?;
        state.end()
    }
}

#[derive(Serialize, Debug)]
pub struct Stats {
    pub web_socket: WebSocketStats,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            web_socket: WebSocketStats::new(),
        }
    }
}
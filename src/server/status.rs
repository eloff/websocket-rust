use axum::{
    response::{IntoResponse, Response, Json},
    http::StatusCode,
    extract::{State, Query}
};

use super::state::ServerState;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct StatusOptions {
    #[serde(default)]
    pub key: String,
}

pub async fn status(
        Query(opts) : Query<StatusOptions>,
        State(state): State<&'static ServerState>,
) -> Response {
    // If status_key is set on state, make sure the user passes a matching key
    // This is a kind of primitive password protection
    if !state.status_key.is_empty() {
        if opts.key != state.status_key {
            return (StatusCode::FORBIDDEN, "key doesn't match STATUS_KEY").into_response();
        }
    }

    Json(&state.stats).into_response()
}
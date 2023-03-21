use axum::{
    response::{IntoResponse, Response, Json},
    http::StatusCode,
    extract::{Query}
};
use serde::Deserialize;

use super::state::get_state;


#[derive(Debug, Deserialize)]
pub struct StatusOptions {
    #[serde(default)]
    pub key: String,
}

pub async fn status(
        Query(opts) : Query<StatusOptions>,
) -> Response {
    let state = get_state();
    // If status_key is set on state, make sure the user passes a matching key
    // This is a kind of primitive password protection
    if !state.status_key.is_empty() {
        if opts.key != state.status_key {
            return (StatusCode::FORBIDDEN, "key doesn't match STATUS_KEY").into_response();
        }
    }

    Json(&state.stats).into_response()
}
//! Authenticated localhost API, WebSocket snapshot stream, and embedded web UI.

mod codex_questions;
mod server;
mod status_messages;

pub use server::{
    ApiServer, ApiServerConfig, ApiServerError, RuntimeRestartHandle, RuntimeRestartRequest,
};

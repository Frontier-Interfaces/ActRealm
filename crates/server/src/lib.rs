//! Authenticated localhost API, WebSocket snapshot stream, and embedded web UI.

mod codex_questions;
mod grok_sessions;
mod kimi_sessions;
mod native_tls;
pub mod native_transport;
mod server;
mod status_messages;

pub use server::{
    ApiServer, ApiServerConfig, ApiServerError, RuntimeRestartHandle, RuntimeRestartRequest,
};

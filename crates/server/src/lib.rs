//! Authenticated localhost API, WebSocket snapshot stream, and embedded web UI.

mod server;
mod status_messages;

pub use server::{
    ApiServer, ApiServerConfig, ApiServerError, RuntimeRestartHandle, RuntimeRestartRequest,
};

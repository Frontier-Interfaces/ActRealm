//! Authenticated localhost API, WebSocket snapshot stream, and embedded web UI.

mod server;

pub use server::{
    ApiServer, ApiServerConfig, ApiServerError, RuntimeRestartHandle, RuntimeRestartRequest,
};

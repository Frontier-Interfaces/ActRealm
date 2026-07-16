#[cfg(unix)]
mod unix;

#[cfg(unix)]
pub use unix::{
    default_socket_path, unix_socket_path_limit, validate_socket_path, BridgeClient, BridgeError,
    BridgeListener, MAX_BRIDGE_FRAME_BYTES,
};

#[cfg(not(unix))]
compile_error!(
    "M0 supports Unix platforms only; Windows loopback transport is scheduled after v1 macOS validation"
);

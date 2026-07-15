#[cfg(unix)]
mod unix;

#[cfg(unix)]
pub use unix::{default_socket_path, BridgeClient, BridgeError, BridgeListener};

#[cfg(not(unix))]
compile_error!(
    "M0 supports Unix platforms only; Windows loopback transport is scheduled after v1 macOS validation"
);

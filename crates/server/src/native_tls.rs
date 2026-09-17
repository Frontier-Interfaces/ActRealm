//! An ephemeral TLS endpoint whose certificate is pinned via the authenticated
//! Unix channel. A process taking over an old TCP port cannot receive tokens.
use axum::Router;
use axum_server::{tls_rustls::RustlsConfig, Handle};
use ring::digest::{digest, SHA256};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::sync::Arc;

pub(crate) struct NativeTls {
    listener: TcpListener,
    config: RustlsConfig,
    pub address: SocketAddr,
    pub certificate_sha256: String,
}

impl NativeTls {
    pub fn new() -> io::Result<Self> {
        let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()])
            .map_err(io::Error::other)?;
        let certificate = certified.cert.der().clone();
        let certificate_sha256 = digest(&SHA256, certificate.as_ref())
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            certified.signing_key.serialize_der(),
        ));
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key)
        .map_err(io::Error::other)?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        Ok(Self {
            listener,
            config: RustlsConfig::from_config(Arc::new(config)),
            address,
            certificate_sha256,
        })
    }

    pub async fn serve(self, router: Router, handle: Handle<SocketAddr>) -> io::Result<()> {
        axum_server::from_tcp_rustls(self.listener, self.config)?
            .handle(handle)
            .serve(router.into_make_service())
            .await
    }
}

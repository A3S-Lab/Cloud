use super::api::{NodeControlApi, PeerCertificate};
use crate::config::NodeControlConfig;
use axum::Extension;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as ConnectionBuilder;
use hyper_util::service::TowerToHyperService;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::BufReader;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;

pub struct NodeControlServer {
    address: SocketAddr,
    tls: Arc<ServerConfig>,
    api: NodeControlApi,
    handshake_timeout: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum NodeControlServerError {
    #[error("invalid node-control address: {0}")]
    Address(String),
    #[error("could not load node-control TLS material: {0}")]
    Tls(String),
    #[error("could not bind node-control listener: {0}")]
    Bind(#[source] std::io::Error),
    #[error("node-control listener failed: {0}")]
    Accept(#[source] std::io::Error),
}

impl NodeControlServer {
    pub(crate) fn from_config(
        config: &NodeControlConfig,
        api: NodeControlApi,
    ) -> Result<Self, NodeControlServerError> {
        let address = format!("{}:{}", config.host, config.port).parse().map_err(
            |error: std::net::AddrParseError| NodeControlServerError::Address(error.to_string()),
        )?;
        let tls = load_tls(
            Path::new(&config.certificate_file),
            Path::new(&config.private_key_file),
            Path::new(&config.client_ca_file),
        )?;
        Ok(Self {
            address,
            tls: Arc::new(tls),
            api,
            handshake_timeout: Duration::from_millis(config.tls_handshake_timeout_ms),
        })
    }

    pub async fn run(
        self,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), NodeControlServerError> {
        let listener = TcpListener::bind(self.address)
            .await
            .map_err(NodeControlServerError::Bind)?;
        let acceptor = TlsAcceptor::from(self.tls);
        let router = self.api.router();
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                accepted = listener.accept() => {
                    let (stream, peer_address) = accepted.map_err(NodeControlServerError::Accept)?;
                    let acceptor = acceptor.clone();
                    let router = router.clone();
                    let handshake_timeout = self.handshake_timeout;
                    connections.spawn(async move {
                        if let Err(error) = serve_connection(
                            stream,
                            peer_address,
                            acceptor,
                            router,
                            handshake_timeout,
                        ).await {
                            tracing::debug!(%peer_address, %error, "node-control connection closed");
                        }
                    });
                }
                completed = connections.join_next(), if !connections.is_empty() => {
                    if let Some(Err(error)) = completed {
                        tracing::warn!(%error, "node-control connection task failed");
                    }
                }
            }
        }
        connections.shutdown().await;
        Ok(())
    }
}

async fn serve_connection(
    stream: TcpStream,
    peer_address: SocketAddr,
    acceptor: TlsAcceptor,
    router: axum::Router,
    handshake_timeout: Duration,
) -> Result<(), String> {
    let tls = tokio::time::timeout(handshake_timeout, acceptor.accept(stream))
        .await
        .map_err(|_| "TLS handshake timed out".to_owned())?
        .map_err(|error| format!("TLS handshake failed: {error}"))?;
    let leaf = tls
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .ok_or_else(|| "verified TLS client did not present a leaf certificate".to_owned())?;
    let peer = PeerCertificate {
        fingerprint: format!("sha256:{:x}", Sha256::digest(leaf.as_ref())),
    };
    let service = TowerToHyperService::new(router.layer(Extension(peer)));
    let io = TokioIo::new(tls);
    let builder = ConnectionBuilder::new(TokioExecutor::new());
    builder
        .serve_connection_with_upgrades(io, service)
        .await
        .map_err(|error| format!("HTTP connection from {peer_address} failed: {error}"))
}

fn load_tls(
    certificate_path: &Path,
    private_key_path: &Path,
    client_ca_path: &Path,
) -> Result<ServerConfig, NodeControlServerError> {
    let certificates = read_certificates(certificate_path)?;
    if certificates.is_empty() {
        return Err(NodeControlServerError::Tls(
            "server certificate chain is empty".into(),
        ));
    }
    let private_key = read_private_key(private_key_path)?;
    let client_roots = read_certificates(client_ca_path)?;
    if client_roots.is_empty() {
        return Err(NodeControlServerError::Tls(
            "client CA bundle is empty".into(),
        ));
    }
    let mut roots = RootCertStore::empty();
    for certificate in client_roots {
        roots.add(certificate).map_err(|error| {
            NodeControlServerError::Tls(format!("client CA certificate is invalid: {error}"))
        })?;
    }
    let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
        .build()
        .map_err(|error| {
            NodeControlServerError::Tls(format!("client certificate verifier is invalid: {error}"))
        })?;
    let builder =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|error| {
                NodeControlServerError::Tls(format!(
                    "TLS protocol configuration is invalid: {error}"
                ))
            })?;
    let mut config = builder
        .with_client_cert_verifier(verifier)
        .with_single_cert(certificates, private_key)
        .map_err(|error| {
            NodeControlServerError::Tls(format!("server certificate identity is invalid: {error}"))
        })?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(config)
}

fn read_certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>, NodeControlServerError> {
    let file = File::open(path).map_err(|error| {
        NodeControlServerError::Tls(format!("could not open {}: {error}", path.display()))
    })?;
    rustls_pemfile::certs(&mut BufReader::new(file))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            NodeControlServerError::Tls(format!(
                "could not parse certificates from {}: {error}",
                path.display()
            ))
        })
}

fn read_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, NodeControlServerError> {
    let file = File::open(path).map_err(|error| {
        NodeControlServerError::Tls(format!("could not open {}: {error}", path.display()))
    })?;
    rustls_pemfile::private_key(&mut BufReader::new(file))
        .map_err(|error| {
            NodeControlServerError::Tls(format!(
                "could not parse private key from {}: {error}",
                path.display()
            ))
        })?
        .ok_or_else(|| {
            NodeControlServerError::Tls(format!("private key file {} is empty", path.display()))
        })
}

use crate::state_file;
use crate::{GatewayCertificateSigningTransport, NodeControlClientError};
use a3s_cloud_contracts::{
    GatewayCertificateRequest, GatewayCertificateSigningRequest, GatewayCertificateSigningResponse,
};
use chrono::{DateTime, Utc};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, KeyPair, SanType,
};
use rustls::client::danger::ServerCertVerifier;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{RootCertStore, ServerConfig};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const CERTIFICATE_FILE: &str = "certificate.pem";
const PRIVATE_KEY_FILE: &str = "private-key.pem";
const CSR_FILE: &str = "request.csr.pem";
const MAX_KEY_BYTES: usize = 64 * 1024;
const MAX_CSR_BYTES: usize = 64 * 1024;
const MAX_CERTIFICATE_BYTES: usize = 512 * 1024;

pub(crate) trait GatewayCertificateClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SystemGatewayCertificateClock;

impl GatewayCertificateClock for SystemGatewayCertificateClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

pub(crate) struct NodeGatewayCertificateProvisioner {
    root: PathBuf,
    node_id: uuid::Uuid,
    transport: Arc<dyn GatewayCertificateSigningTransport>,
    clock: Arc<dyn GatewayCertificateClock>,
}

impl NodeGatewayCertificateProvisioner {
    pub(crate) fn new(
        root: PathBuf,
        node_id: uuid::Uuid,
        transport: Arc<dyn GatewayCertificateSigningTransport>,
        clock: Arc<dyn GatewayCertificateClock>,
    ) -> Result<Self, GatewayCertificateProvisioningError> {
        if !root.is_absolute()
            || root
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
            || node_id.is_nil()
        {
            return Err(GatewayCertificateProvisioningError::Invalid(
                "managed Gateway certificate identity or directory is invalid".into(),
            ));
        }
        Ok(Self {
            root,
            node_id,
            transport,
            clock,
        })
    }

    pub(crate) async fn provision(
        &self,
        request: &GatewayCertificateRequest,
    ) -> Result<(), GatewayCertificateProvisioningError> {
        request
            .validate()
            .map_err(GatewayCertificateProvisioningError::Invalid)?;
        let paths = ManagedCertificatePaths::new(&self.root, request)?;
        let request_for_storage = request.clone();
        let prepared =
            tokio::task::spawn_blocking(move || prepare_certificate(&paths, &request_for_storage))
                .await
                .map_err(|error| {
                    GatewayCertificateProvisioningError::Storage(format!(
                        "Gateway certificate preparation task failed: {error}"
                    ))
                })??;
        let PreparedCertificate::Pending {
            paths,
            private_key_pem,
            csr_pem,
        } = prepared
        else {
            return Ok(());
        };
        let signing_request = GatewayCertificateSigningRequest {
            schema: GatewayCertificateSigningRequest::SCHEMA.into(),
            certificate_id: request.certificate_id,
            node_id: self.node_id,
            csr_pem,
            requested_at: self.clock.now(),
        };
        signing_request
            .validate()
            .map_err(GatewayCertificateProvisioningError::Invalid)?;
        let response = self
            .transport
            .sign_gateway_certificate(&signing_request)
            .await
            .map_err(GatewayCertificateProvisioningError::ControlPlane)?;
        if response.node_id != self.node_id {
            return Err(GatewayCertificateProvisioningError::Invalid(
                "Gateway certificate response changed the node identity".into(),
            ));
        }
        let request_for_verification = request.clone();
        tokio::task::spawn_blocking(move || {
            verify_and_store_certificate(
                &paths,
                &request_for_verification,
                &private_key_pem,
                &response,
            )
        })
        .await
        .map_err(|error| {
            GatewayCertificateProvisioningError::Storage(format!(
                "Gateway certificate verification task failed: {error}"
            ))
        })?
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum GatewayCertificateProvisioningError {
    #[error("invalid managed Gateway certificate: {0}")]
    Invalid(String),
    #[error("managed Gateway certificate storage failed: {0}")]
    Storage(String),
    #[error(transparent)]
    ControlPlane(#[from] NodeControlClientError),
}

struct ManagedCertificatePaths {
    directory: PathBuf,
    certificate: PathBuf,
    private_key: PathBuf,
    csr: PathBuf,
}

impl ManagedCertificatePaths {
    fn new(
        root: &Path,
        request: &GatewayCertificateRequest,
    ) -> Result<Self, GatewayCertificateProvisioningError> {
        let directory = root.join(request.certificate_id.to_string());
        let certificate = directory.join(CERTIFICATE_FILE);
        let private_key = directory.join(PRIVATE_KEY_FILE);
        if request.certificate_file != certificate.to_string_lossy()
            || request.private_key_file != private_key.to_string_lossy()
        {
            return Err(GatewayCertificateProvisioningError::Invalid(
                "Gateway snapshot certificate paths are outside the managed certificate directory"
                    .into(),
            ));
        }
        Ok(Self {
            csr: directory.join(CSR_FILE),
            directory,
            certificate,
            private_key,
        })
    }
}

enum PreparedCertificate {
    Ready,
    Pending {
        paths: ManagedCertificatePaths,
        private_key_pem: String,
        csr_pem: String,
    },
}

fn prepare_certificate(
    paths: &ManagedCertificatePaths,
    request: &GatewayCertificateRequest,
) -> Result<PreparedCertificate, GatewayCertificateProvisioningError> {
    let root = paths.directory.parent().ok_or_else(|| {
        GatewayCertificateProvisioningError::Invalid(
            "managed Gateway certificate directory has no root".into(),
        )
    })?;
    state_file::ensure_directory(root).map_err(map_state_error)?;
    state_file::ensure_directory(&paths.directory).map_err(map_state_error)?;
    if paths.certificate.exists() {
        if !paths.private_key.exists() || !paths.csr.exists() {
            return Err(GatewayCertificateProvisioningError::Invalid(
                "managed Gateway certificate is missing its private key or CSR".into(),
            ));
        }
        let private_key_pem =
            read_bounded(&paths.private_key, MAX_KEY_BYTES, "Gateway private key")?;
        let certificate_pem = read_bounded(
            &paths.certificate,
            MAX_CERTIFICATE_BYTES,
            "Gateway certificate",
        )?;
        verify_stored_certificate(request, &private_key_pem, &certificate_pem)?;
        return Ok(PreparedCertificate::Ready);
    }
    if paths.csr.exists() && !paths.private_key.exists() {
        return Err(GatewayCertificateProvisioningError::Invalid(
            "managed Gateway CSR has no private key".into(),
        ));
    }
    let private_key_pem = if paths.private_key.exists() {
        let pem = read_bounded(&paths.private_key, MAX_KEY_BYTES, "Gateway private key")?;
        KeyPair::from_pem(&pem).map_err(|error| {
            GatewayCertificateProvisioningError::Invalid(format!(
                "managed Gateway private key is invalid: {error}"
            ))
        })?;
        pem
    } else {
        let key = KeyPair::generate().map_err(|error| {
            GatewayCertificateProvisioningError::Storage(format!(
                "could not generate Gateway private key: {error}"
            ))
        })?;
        let pem = key.serialize_pem();
        atomic_write_new(&paths.private_key, pem.as_bytes(), 0o600)?;
        pem
    };
    let csr_pem = if paths.csr.exists() {
        let pem = read_bounded(&paths.csr, MAX_CSR_BYTES, "Gateway CSR")?;
        validate_csr(request, &pem)?;
        pem
    } else {
        let key = KeyPair::from_pem(&private_key_pem).map_err(|error| {
            GatewayCertificateProvisioningError::Invalid(format!(
                "managed Gateway private key is invalid: {error}"
            ))
        })?;
        let mut params = CertificateParams::new(request.dns_names.clone()).map_err(|error| {
            GatewayCertificateProvisioningError::Invalid(format!(
                "Gateway certificate DNS names are invalid: {error}"
            ))
        })?;
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::OrganizationName, "A3S Cloud");
        distinguished_name.push(
            DnType::CommonName,
            format!("a3s-gateway:{}", request.certificate_id),
        );
        params.distinguished_name = distinguished_name;
        let pem = params
            .serialize_request(&key)
            .and_then(|request| request.pem())
            .map_err(|error| {
                GatewayCertificateProvisioningError::Invalid(format!(
                    "could not create Gateway CSR: {error}"
                ))
            })?;
        validate_csr(request, &pem)?;
        atomic_write_new(&paths.csr, pem.as_bytes(), 0o600)?;
        pem
    };
    Ok(PreparedCertificate::Pending {
        paths: ManagedCertificatePaths {
            directory: paths.directory.clone(),
            certificate: paths.certificate.clone(),
            private_key: paths.private_key.clone(),
            csr: paths.csr.clone(),
        },
        private_key_pem,
        csr_pem,
    })
}

fn validate_csr(
    request: &GatewayCertificateRequest,
    csr_pem: &str,
) -> Result<(), GatewayCertificateProvisioningError> {
    GatewayCertificateSigningRequest {
        schema: GatewayCertificateSigningRequest::SCHEMA.into(),
        certificate_id: request.certificate_id,
        node_id: uuid::Uuid::now_v7(),
        csr_pem: csr_pem.into(),
        requested_at: Utc::now(),
    }
    .validate()
    .map_err(GatewayCertificateProvisioningError::Invalid)
}

fn verify_and_store_certificate(
    paths: &ManagedCertificatePaths,
    request: &GatewayCertificateRequest,
    private_key_pem: &str,
    response: &GatewayCertificateSigningResponse,
) -> Result<(), GatewayCertificateProvisioningError> {
    response
        .validate()
        .map_err(GatewayCertificateProvisioningError::Invalid)?;
    if response.certificate_id != request.certificate_id || response.dns_names != request.dns_names
    {
        return Err(GatewayCertificateProvisioningError::Invalid(
            "Gateway certificate response changed the requested identity or DNS names".into(),
        ));
    }
    verify_certificate_response(request, private_key_pem, response)?;
    let certificate_chain = format!("{}{}", response.certificate_pem, response.ca_bundle_pem);
    atomic_write_new(&paths.certificate, certificate_chain.as_bytes(), 0o644)
}

fn verify_stored_certificate(
    request: &GatewayCertificateRequest,
    private_key_pem: &str,
    certificate_chain_pem: &str,
) -> Result<(), GatewayCertificateProvisioningError> {
    let certificates = parse_certificates(certificate_chain_pem)?;
    let (leaf, authority) = certificates.split_first().ok_or_else(|| {
        GatewayCertificateProvisioningError::Invalid(
            "managed Gateway certificate chain is empty".into(),
        )
    })?;
    if authority.is_empty() {
        return Err(GatewayCertificateProvisioningError::Invalid(
            "managed Gateway certificate chain has no CA bundle".into(),
        ));
    }
    verify_certificate(request, private_key_pem, leaf, authority, None, None)
}

fn verify_certificate_response(
    request: &GatewayCertificateRequest,
    private_key_pem: &str,
    response: &GatewayCertificateSigningResponse,
) -> Result<(), GatewayCertificateProvisioningError> {
    let leaf = parse_certificates(&response.certificate_pem)?;
    if leaf.len() != 1 {
        return Err(GatewayCertificateProvisioningError::Invalid(
            "Gateway certificate response must contain exactly one leaf certificate".into(),
        ));
    }
    let authority = parse_certificates(&response.ca_bundle_pem)?;
    if authority.is_empty() {
        return Err(GatewayCertificateProvisioningError::Invalid(
            "Gateway certificate response has an empty CA bundle".into(),
        ));
    }
    let fingerprint = format!("sha256:{:x}", Sha256::digest(leaf[0].as_ref()));
    if fingerprint != response.fingerprint {
        return Err(GatewayCertificateProvisioningError::Invalid(
            "Gateway certificate fingerprint does not match the leaf certificate".into(),
        ));
    }
    verify_certificate(
        request,
        private_key_pem,
        &leaf[0],
        &authority,
        Some(response.issued_at),
        Some(response.expires_at),
    )?;
    let params = CertificateParams::from_ca_cert_der(&leaf[0]).map_err(|error| {
        GatewayCertificateProvisioningError::Invalid(format!(
            "Gateway certificate is invalid: {error}"
        ))
    })?;
    if params
        .serial_number
        .as_ref()
        .map(ToString::to_string)
        .as_deref()
        != Some(response.serial_number.as_str())
    {
        return Err(GatewayCertificateProvisioningError::Invalid(
            "Gateway certificate serial number does not match the response".into(),
        ));
    }
    Ok(())
}

fn verify_certificate(
    request: &GatewayCertificateRequest,
    private_key_pem: &str,
    leaf: &CertificateDer<'_>,
    authority: &[CertificateDer<'_>],
    issued_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
) -> Result<(), GatewayCertificateProvisioningError> {
    let params = CertificateParams::from_ca_cert_der(leaf).map_err(|error| {
        GatewayCertificateProvisioningError::Invalid(format!(
            "Gateway certificate is invalid: {error}"
        ))
    })?;
    if !matches!(params.is_ca, rcgen::IsCa::NoCa | rcgen::IsCa::ExplicitNoCa)
        || !has_exact_dns_names(&params.subject_alt_names, &request.dns_names)
        || params.extended_key_usages != vec![ExtendedKeyUsagePurpose::ServerAuth]
    {
        return Err(GatewayCertificateProvisioningError::Invalid(
            "Gateway certificate SAN or usage identity is invalid".into(),
        ));
    }
    if let (Some(issued_at), Some(expires_at)) = (issued_at, expires_at) {
        if params.not_before.unix_timestamp() != issued_at.timestamp()
            || params.not_after.unix_timestamp() != expires_at.timestamp()
        {
            return Err(GatewayCertificateProvisioningError::Invalid(
                "Gateway certificate validity does not match the response".into(),
            ));
        }
    }
    verify_private_key(
        CertificateDer::from(leaf.as_ref().to_vec()),
        private_key_pem,
    )?;
    verify_chain_and_dns(request, leaf, authority, issued_at)
}

fn has_exact_dns_names(subject_alt_names: &[SanType], expected: &[String]) -> bool {
    if subject_alt_names.len() != expected.len() {
        return false;
    }
    let mut actual = Vec::with_capacity(subject_alt_names.len());
    for subject_alt_name in subject_alt_names {
        let SanType::DnsName(dns_name) = subject_alt_name else {
            return false;
        };
        actual.push(dns_name.as_str());
    }
    actual.sort_unstable();
    actual
        .iter()
        .copied()
        .zip(expected.iter().map(String::as_str))
        .all(|(actual, expected)| actual == expected)
}

fn verify_private_key(
    leaf: CertificateDer<'static>,
    private_key_pem: &str,
) -> Result<(), GatewayCertificateProvisioningError> {
    let private_key = parse_private_key(private_key_pem)?;
    ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|error| {
            GatewayCertificateProvisioningError::Invalid(format!(
                "Gateway TLS protocol configuration is invalid: {error}"
            ))
        })?
        .with_no_client_auth()
        .with_single_cert(vec![leaf], private_key)
        .map_err(|_| {
            GatewayCertificateProvisioningError::Invalid(
                "Gateway certificate does not match its managed private key".into(),
            )
        })?;
    Ok(())
}

fn verify_chain_and_dns(
    request: &GatewayCertificateRequest,
    leaf: &CertificateDer<'_>,
    authority: &[CertificateDer<'_>],
    issued_at: Option<DateTime<Utc>>,
) -> Result<(), GatewayCertificateProvisioningError> {
    let mut roots = RootCertStore::empty();
    for certificate in authority {
        roots.add(certificate.clone()).map_err(|error| {
            GatewayCertificateProvisioningError::Invalid(format!(
                "Gateway CA bundle is invalid: {error}"
            ))
        })?;
    }
    let verifier = rustls::client::WebPkiServerVerifier::builder(Arc::new(roots))
        .build()
        .map_err(|error| {
            GatewayCertificateProvisioningError::Invalid(format!(
                "Gateway CA verifier is invalid: {error}"
            ))
        })?;
    let dns_name = representative_dns_name(&request.dns_names[0]);
    let server_name = ServerName::try_from(dns_name).map_err(|error| {
        GatewayCertificateProvisioningError::Invalid(format!(
            "Gateway certificate DNS name is invalid: {error}"
        ))
    })?;
    let verification_time = issued_at
        .unwrap_or_else(Utc::now)
        .timestamp()
        .saturating_add(1);
    let verification_time = u64::try_from(verification_time).map_err(|_| {
        GatewayCertificateProvisioningError::Invalid(
            "Gateway certificate verification time is invalid".into(),
        )
    })?;
    verifier
        .verify_server_cert(
            leaf,
            &[],
            &server_name,
            &[],
            UnixTime::since_unix_epoch(std::time::Duration::from_secs(verification_time)),
        )
        .map_err(|error| {
            GatewayCertificateProvisioningError::Invalid(format!(
                "Gateway certificate chain or DNS identity is invalid: {error}"
            ))
        })?;
    Ok(())
}

fn representative_dns_name(pattern: &str) -> String {
    pattern
        .strip_prefix("*.")
        .map(|suffix| format!("a3s-validation.{suffix}"))
        .unwrap_or_else(|| pattern.into())
}

fn parse_certificates(
    pem: &str,
) -> Result<Vec<CertificateDer<'static>>, GatewayCertificateProvisioningError> {
    rustls_pemfile::certs(&mut BufReader::new(pem.as_bytes()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            GatewayCertificateProvisioningError::Invalid(format!(
                "Gateway certificate PEM is invalid: {error}"
            ))
        })
}

fn parse_private_key(
    pem: &str,
) -> Result<PrivateKeyDer<'static>, GatewayCertificateProvisioningError> {
    rustls_pemfile::private_key(&mut BufReader::new(pem.as_bytes()))
        .map_err(|error| {
            GatewayCertificateProvisioningError::Invalid(format!(
                "Gateway private key PEM is invalid: {error}"
            ))
        })?
        .ok_or_else(|| {
            GatewayCertificateProvisioningError::Invalid("Gateway private key PEM is empty".into())
        })
}

fn read_bounded(
    path: &Path,
    maximum: usize,
    label: &str,
) -> Result<String, GatewayCertificateProvisioningError> {
    let metadata = std::fs::symlink_metadata(path).map_err(storage("inspect managed file"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > maximum as u64 {
        return Err(GatewayCertificateProvisioningError::Invalid(format!(
            "{label} is not a bounded regular file"
        )));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut bytes = Vec::new();
    options
        .open(path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(storage("read managed file"))?;
    if bytes.len() > maximum {
        return Err(GatewayCertificateProvisioningError::Invalid(format!(
            "{label} exceeds its size limit"
        )));
    }
    String::from_utf8(bytes).map_err(|error| {
        GatewayCertificateProvisioningError::Invalid(format!("{label} is not UTF-8: {error}"))
    })
}

fn atomic_write_new(
    path: &Path,
    bytes: &[u8],
    mode: u32,
) -> Result<(), GatewayCertificateProvisioningError> {
    if path.exists() {
        return Err(GatewayCertificateProvisioningError::Invalid(format!(
            "managed Gateway file {} already exists",
            path.display()
        )));
    }
    let parent = path.parent().ok_or_else(|| {
        GatewayCertificateProvisioningError::Invalid("managed Gateway file has no parent".into())
    })?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(storage("create managed staging file"))?;
    temporary
        .write_all(bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(storage("write managed staging file"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(mode))
            .map_err(storage("secure managed staging file"))?;
    }
    temporary
        .persist_noclobber(path)
        .map_err(|error| storage("publish managed Gateway file")(error.error))?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(storage("sync managed Gateway directory"))?;
    Ok(())
}

fn map_state_error(error: state_file::SecureStateError) -> GatewayCertificateProvisioningError {
    match error {
        state_file::SecureStateError::Invalid(message) => {
            GatewayCertificateProvisioningError::Invalid(message)
        }
        state_file::SecureStateError::Storage(message) => {
            GatewayCertificateProvisioningError::Storage(message)
        }
    }
}

fn storage(
    action: &'static str,
) -> impl FnOnce(std::io::Error) -> GatewayCertificateProvisioningError {
    move |error| {
        GatewayCertificateProvisioningError::Storage(format!("could not {action}: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::TimeZone;
    use rcgen::{
        BasicConstraints, Certificate, CertificateSigningRequestParams, IsCa, KeyUsagePurpose,
        SerialNumber,
    };
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct FixedClock(DateTime<Utc>);

    impl GatewayCertificateClock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }

    struct TestSigningTransport {
        node_id: uuid::Uuid,
        dns_names: Vec<String>,
        certificate: Certificate,
        certificate_pem: String,
        private_key: KeyPair,
        calls: AtomicUsize,
        change_identity: AtomicBool,
    }

    impl TestSigningTransport {
        fn new(node_id: uuid::Uuid, dns_names: Vec<String>) -> Self {
            let private_key = KeyPair::generate().expect("test CA key");
            let mut params = CertificateParams::default();
            params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
            params.key_usages = vec![
                KeyUsagePurpose::DigitalSignature,
                KeyUsagePurpose::KeyCertSign,
                KeyUsagePurpose::CrlSign,
            ];
            let certificate = params.self_signed(&private_key).expect("test CA");
            let certificate_pem = certificate.pem();
            Self {
                node_id,
                dns_names,
                certificate,
                certificate_pem,
                private_key,
                calls: AtomicUsize::new(0),
                change_identity: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl GatewayCertificateSigningTransport for TestSigningTransport {
        async fn sign_gateway_certificate(
            &self,
            request: &GatewayCertificateSigningRequest,
        ) -> Result<GatewayCertificateSigningResponse, NodeControlClientError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            request
                .validate()
                .map_err(NodeControlClientError::Invalid)?;
            if request.node_id != self.node_id {
                return Err(NodeControlClientError::Invalid(
                    "test signing node changed".into(),
                ));
            }
            let mut csr = CertificateSigningRequestParams::from_pem(&request.csr_pem)
                .map_err(|error| NodeControlClientError::Invalid(error.to_string()))?;
            let serial = SerialNumber::from_slice(b"provider-serial");
            csr.params.serial_number = Some(serial.clone());
            csr.params.is_ca = IsCa::NoCa;
            csr.params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
            csr.params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
            csr.params.subject_alt_names = self
                .dns_names
                .iter()
                .rev()
                .map(|dns_name| {
                    dns_name
                        .as_str()
                        .try_into()
                        .map(SanType::DnsName)
                        .map_err(|error| NodeControlClientError::Invalid(error.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let issued_at = Utc
                .timestamp_opt(csr.params.not_before.unix_timestamp(), 0)
                .single()
                .ok_or_else(|| {
                    NodeControlClientError::Invalid("test issue timestamp is invalid".into())
                })?;
            let expires_at = Utc
                .timestamp_opt(csr.params.not_after.unix_timestamp(), 0)
                .single()
                .ok_or_else(|| {
                    NodeControlClientError::Invalid("test expiry timestamp is invalid".into())
                })?;
            let certificate = csr
                .signed_by(&self.certificate, &self.private_key)
                .map_err(|error| NodeControlClientError::Invalid(error.to_string()))?;
            let dns_names = if self.change_identity.load(Ordering::SeqCst) {
                vec!["changed.example.net".into()]
            } else {
                self.dns_names.clone()
            };
            Ok(GatewayCertificateSigningResponse {
                schema: GatewayCertificateSigningResponse::SCHEMA.into(),
                certificate_id: request.certificate_id,
                node_id: request.node_id,
                dns_names,
                serial_number: serial.to_string(),
                fingerprint: format!("sha256:{:x}", Sha256::digest(certificate.der())),
                certificate_pem: certificate.pem(),
                ca_bundle_pem: self.certificate_pem.clone(),
                issued_at,
                expires_at,
            })
        }
    }

    fn certificate_request(
        root: &Path,
        certificate_id: uuid::Uuid,
        dns_names: Vec<String>,
    ) -> GatewayCertificateRequest {
        let directory = root.join(certificate_id.to_string());
        GatewayCertificateRequest::new(
            certificate_id,
            dns_names,
            directory.join(CERTIFICATE_FILE).to_string_lossy(),
            directory.join(PRIVATE_KEY_FILE).to_string_lossy(),
        )
        .expect("Gateway certificate request")
    }

    #[tokio::test]
    async fn persists_the_node_key_and_csr_before_an_idempotent_signed_certificate() {
        let directory = tempfile::tempdir().expect("managed certificate root");
        let root = directory.path().join("certificates");
        let node_id = uuid::Uuid::now_v7();
        let dns_names = vec!["*.example.com".into(), "api.example.net".into()];
        let transport = Arc::new(TestSigningTransport::new(node_id, dns_names.clone()));
        let provisioner = NodeGatewayCertificateProvisioner::new(
            root.clone(),
            node_id,
            transport.clone(),
            Arc::new(FixedClock(Utc::now())),
        )
        .expect("certificate provisioner");
        let certificate_id = uuid::Uuid::now_v7();
        let request = certificate_request(&root, certificate_id, dns_names);
        provisioner
            .provision(&request)
            .await
            .expect("provision certificate");
        let managed = root.join(certificate_id.to_string());
        let private_key =
            std::fs::read_to_string(managed.join(PRIVATE_KEY_FILE)).expect("private key");
        let csr = std::fs::read_to_string(managed.join(CSR_FILE)).expect("CSR");
        let certificate =
            std::fs::read_to_string(managed.join(CERTIFICATE_FILE)).expect("certificate chain");
        assert!(private_key.contains("PRIVATE KEY"));
        assert!(!csr.contains("PRIVATE KEY"));
        assert!(!certificate.contains("PRIVATE KEY"));
        assert_eq!(parse_certificates(&certificate).expect("chain").len(), 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(managed.join(PRIVATE_KEY_FILE))
                    .expect("key metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        provisioner.provision(&request).await.expect("local replay");
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn rejects_a_changed_signing_identity_before_publishing_the_certificate() {
        let directory = tempfile::tempdir().expect("managed certificate root");
        let root = directory.path().join("certificates");
        let node_id = uuid::Uuid::now_v7();
        let dns_names = vec!["api.example.com".into()];
        let transport = Arc::new(TestSigningTransport::new(node_id, dns_names.clone()));
        transport.change_identity.store(true, Ordering::SeqCst);
        let provisioner = NodeGatewayCertificateProvisioner::new(
            root.clone(),
            node_id,
            transport,
            Arc::new(FixedClock(Utc::now())),
        )
        .expect("certificate provisioner");
        let certificate_id = uuid::Uuid::now_v7();
        let request = certificate_request(&root, certificate_id, dns_names);
        let error = provisioner
            .provision(&request)
            .await
            .expect_err("changed identity");
        assert!(matches!(
            error,
            GatewayCertificateProvisioningError::Invalid(_)
        ));
        let managed = root.join(certificate_id.to_string());
        assert!(managed.join(PRIVATE_KEY_FILE).is_file());
        assert!(managed.join(CSR_FILE).is_file());
        assert!(!managed.join(CERTIFICATE_FILE).exists());
    }
}

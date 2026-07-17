use crate::modules::fleet::domain::entities::{NodeCertificate, NodeCertificateMaterial};
use crate::modules::fleet::domain::services::{
    CertificateAuthorityError, ICertificateAuthority, NodeCertificateRequest,
};
use async_trait::async_trait;
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, CertificateSigningRequestParams,
    DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SanType,
    SerialNumber,
};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use time::OffsetDateTime;
use uuid::Uuid;

const CA_CERTIFICATE_FILE: &str = "ca.pem";
const CA_PRIVATE_KEY_FILE: &str = "ca-key.pem";
const REVOCATION_FILE: &str = "revoked-serials";

pub struct LocalCertificateAuthority {
    root: PathBuf,
    certificate: Certificate,
    certificate_pem: String,
    private_key: KeyPair,
    mutation_lock: Mutex<()>,
}

impl LocalCertificateAuthority {
    pub fn load_or_create(root: impl Into<PathBuf>) -> Result<Self, CertificateAuthorityError> {
        let root = root.into();
        ensure_private_directory(&root)?;
        let certificate_path = root.join(CA_CERTIFICATE_FILE);
        let key_path = root.join(CA_PRIVATE_KEY_FILE);
        let (certificate_pem, private_key) =
            match (certificate_path.exists(), key_path.exists()) {
                (true, true) => {
                    let certificate_pem = fs::read_to_string(&certificate_path)
                        .map_err(unavailable("read local CA certificate"))?;
                    let key_pem = fs::read_to_string(&key_path)
                        .map_err(unavailable("read local CA private key"))?;
                    let private_key = KeyPair::from_pem(&key_pem).map_err(|error| {
                        CertificateAuthorityError::Rejected(format!(
                            "local CA private key is invalid: {error}"
                        ))
                    })?;
                    (certificate_pem, private_key)
                }
                (false, false) => create_ca_material(&root)?,
                _ => return Err(CertificateAuthorityError::Rejected(
                    "local CA certificate and private key must either both exist or both be absent"
                        .into(),
                )),
            };
        let params = CertificateParams::from_ca_cert_pem(&certificate_pem).map_err(|error| {
            CertificateAuthorityError::Rejected(format!("local CA certificate is invalid: {error}"))
        })?;
        let certificate = params.self_signed(&private_key).map_err(|error| {
            CertificateAuthorityError::Rejected(format!(
                "local CA certificate and key do not form a signing identity: {error}"
            ))
        })?;
        Ok(Self {
            root,
            certificate,
            certificate_pem,
            private_key,
            mutation_lock: Mutex::new(()),
        })
    }

    pub fn ensure_server_identity(
        &self,
        server_name: &str,
        certificate_path: &Path,
        private_key_path: &Path,
    ) -> Result<(), CertificateAuthorityError> {
        if server_name.trim().is_empty()
            || server_name.len() > 255
            || server_name.contains(['\0', '\r', '\n'])
            || certificate_path == private_key_path
        {
            return Err(CertificateAuthorityError::InvalidRequest(
                "node-control server identity configuration is invalid".into(),
            ));
        }
        match (certificate_path.exists(), private_key_path.exists()) {
            (true, true) => return Ok(()),
            (true, false) | (false, true) => {
                return Err(CertificateAuthorityError::Rejected(
                    "node-control certificate and private key must both exist or both be absent"
                        .into(),
                ))
            }
            (false, false) => {}
        }
        let certificate_parent = certificate_path.parent().ok_or_else(|| {
            CertificateAuthorityError::InvalidRequest(
                "node-control certificate path has no parent".into(),
            )
        })?;
        let key_parent = private_key_path.parent().ok_or_else(|| {
            CertificateAuthorityError::InvalidRequest("node-control key path has no parent".into())
        })?;
        ensure_private_directory(certificate_parent)?;
        ensure_private_directory(key_parent)?;

        let private_key = KeyPair::generate().map_err(|error| {
            CertificateAuthorityError::Unavailable(format!(
                "could not generate node-control server key: {error}"
            ))
        })?;
        let mut params = CertificateParams::default();
        let now = OffsetDateTime::now_utc();
        params.not_before = now - time::Duration::minutes(5);
        params.not_after = now + time::Duration::days(365);
        params.serial_number = Some(SerialNumber::from_slice(Uuid::now_v7().as_bytes()));
        params.is_ca = IsCa::NoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.subject_alt_names = if let Ok(address) = server_name.parse::<IpAddr>() {
            vec![SanType::IpAddress(address)]
        } else {
            vec![SanType::DnsName(server_name.try_into().map_err(
                |error| {
                    CertificateAuthorityError::InvalidRequest(format!(
                        "node-control server name is invalid: {error}"
                    ))
                },
            )?)]
        };
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::OrganizationName, "A3S Cloud");
        distinguished_name.push(DnType::CommonName, server_name);
        params.distinguished_name = distinguished_name;
        let certificate = params
            .signed_by(&private_key, &self.certificate, &self.private_key)
            .map_err(|error| {
                CertificateAuthorityError::Rejected(format!(
                    "could not sign node-control server certificate: {error}"
                ))
            })?;
        private_write(private_key_path, &private_key.serialize_pem())?;
        public_write(certificate_path, &certificate.pem())
    }

    pub fn ensure_ca_bundle(&self, path: &Path) -> Result<(), CertificateAuthorityError> {
        if path.exists() {
            let existing = fs::read_to_string(path)
                .map_err(unavailable("read node-control client CA bundle"))?;
            return if existing == self.certificate_pem {
                Ok(())
            } else {
                Err(CertificateAuthorityError::Rejected(
                    "node-control client CA bundle does not match the local node CA".into(),
                ))
            };
        }
        let parent = path.parent().ok_or_else(|| {
            CertificateAuthorityError::InvalidRequest(
                "node-control client CA path has no parent".into(),
            )
        })?;
        ensure_private_directory(parent)?;
        public_write(path, &self.certificate_pem)
    }
}

#[async_trait]
impl ICertificateAuthority for LocalCertificateAuthority {
    async fn issue(
        &self,
        request: NodeCertificateRequest,
    ) -> Result<NodeCertificate, CertificateAuthorityError> {
        if request.expires_at <= request.issued_at {
            return Err(CertificateAuthorityError::InvalidRequest(
                "certificate expiry must follow issue time".into(),
            ));
        }
        let mut csr =
            CertificateSigningRequestParams::from_pem(&request.csr_pem).map_err(|error| {
                CertificateAuthorityError::InvalidRequest(format!(
                    "certificate signing request is invalid: {error}"
                ))
            })?;
        let serial = SerialNumber::from_slice(request.certificate_id.as_uuid().as_bytes());
        csr.params.not_before = timestamp(request.issued_at.timestamp())?;
        csr.params.not_after = timestamp(request.expires_at.timestamp())?;
        csr.params.serial_number = Some(serial.clone());
        csr.params.is_ca = IsCa::NoCa;
        csr.params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        csr.params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::OrganizationName, "A3S Cloud");
        distinguished_name.push(DnType::CommonName, format!("a3s-node:{}", request.node_id));
        csr.params.distinguished_name = distinguished_name;
        let identity = format!("spiffe://a3s.cloud/nodes/{}", request.node_id)
            .try_into()
            .map_err(|error| {
                CertificateAuthorityError::InvalidRequest(format!(
                    "node certificate identity is invalid: {error}"
                ))
            })?;
        csr.params.subject_alt_names = vec![SanType::URI(identity)];
        let certificate = csr
            .signed_by(&self.certificate, &self.private_key)
            .map_err(|error| CertificateAuthorityError::Rejected(error.to_string()))?;
        NodeCertificate::new(
            request.certificate_id,
            request.node_id,
            NodeCertificateMaterial {
                serial_number: serial.to_string(),
                fingerprint: format!("sha256:{:x}", Sha256::digest(certificate.der())),
                certificate_pem: certificate.pem(),
                ca_bundle_pem: self.certificate_pem.clone(),
                issued_at: request.issued_at,
                expires_at: request.expires_at,
            },
        )
        .map_err(CertificateAuthorityError::InvalidRequest)
    }

    async fn revoke(&self, certificate: &NodeCertificate) -> Result<(), CertificateAuthorityError> {
        let _guard = self
            .mutation_lock
            .lock()
            .map_err(|_| CertificateAuthorityError::Unavailable("local CA lock poisoned".into()))?;
        let path = self.root.join(REVOCATION_FILE);
        let existing = match fs::read_to_string(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(unavailable("read local CA revocations")(error)),
        };
        if existing
            .lines()
            .any(|line| line == certificate.serial_number)
        {
            return Ok(());
        }
        let mut file = private_append(&path)?;
        writeln!(file, "{}", certificate.serial_number)
            .map_err(unavailable("record local CA revocation"))?;
        file.sync_all()
            .map_err(unavailable("sync local CA revocation"))
    }

    async fn health(&self) -> Result<bool, CertificateAuthorityError> {
        Ok(self.root.join(CA_CERTIFICATE_FILE).is_file()
            && self.root.join(CA_PRIVATE_KEY_FILE).is_file())
    }
}

fn create_ca_material(root: &Path) -> Result<(String, KeyPair), CertificateAuthorityError> {
    let private_key = KeyPair::generate().map_err(|error| {
        CertificateAuthorityError::Unavailable(format!("could not generate local CA key: {error}"))
    })?;
    let mut params = CertificateParams::default();
    let now = OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::minutes(5);
    params.not_after = now + time::Duration::days(3650);
    params.serial_number = Some(SerialNumber::from_slice(uuid::Uuid::now_v7().as_bytes()));
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::OrganizationName, "A3S Cloud Development");
    distinguished_name.push(DnType::CommonName, "A3S Cloud Local Node CA");
    params.distinguished_name = distinguished_name;
    let certificate = params.self_signed(&private_key).map_err(|error| {
        CertificateAuthorityError::Unavailable(format!(
            "could not generate local CA certificate: {error}"
        ))
    })?;
    let certificate_pem = certificate.pem();
    private_write(
        &root.join(CA_PRIVATE_KEY_FILE),
        &private_key.serialize_pem(),
    )?;
    public_write(&root.join(CA_CERTIFICATE_FILE), &certificate_pem)?;
    Ok((certificate_pem, private_key))
}

fn timestamp(value: i64) -> Result<OffsetDateTime, CertificateAuthorityError> {
    OffsetDateTime::from_unix_timestamp(value).map_err(|error| {
        CertificateAuthorityError::InvalidRequest(format!(
            "certificate timestamp is out of range: {error}"
        ))
    })
}

fn ensure_private_directory(path: &Path) -> Result<(), CertificateAuthorityError> {
    fs::create_dir_all(path).map_err(unavailable("create local CA directory"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(unavailable("secure local CA directory"))?;
    }
    Ok(())
}

fn private_write(path: &Path, value: &str) -> Result<(), CertificateAuthorityError> {
    write_new(path, value, true)
}

fn public_write(path: &Path, value: &str) -> Result<(), CertificateAuthorityError> {
    write_new(path, value, false)
}

fn write_new(path: &Path, value: &str, private: bool) -> Result<(), CertificateAuthorityError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if private { 0o600 } else { 0o644 });
    }
    let mut file = options
        .open(path)
        .map_err(unavailable("create local CA file"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(if private {
            0o600
        } else {
            0o644
        }))
        .map_err(unavailable("set local CA file permissions"))?;
    }
    file.write_all(value.as_bytes())
        .map_err(unavailable("write local CA file"))?;
    file.sync_all().map_err(unavailable("sync local CA file"))
}

fn private_append(path: &Path) -> Result<std::fs::File, CertificateAuthorityError> {
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(unavailable("open local CA file"))
}

fn unavailable(action: &'static str) -> impl FnOnce(std::io::Error) -> CertificateAuthorityError {
    move |error| CertificateAuthorityError::Unavailable(format!("{action}: {error}"))
}

use crate::modules::edge::domain::services::{
    GatewayCertificateAuthorityError, GatewayCertificateIssueRequest, IGatewayCertificateAuthority,
};
use crate::modules::edge::domain::{GatewayCertificate, GatewayCertificateMaterial};
use async_trait::async_trait;
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, CertificateSigningRequestParams,
    DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, SanType,
    SerialNumber,
};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use time::OffsetDateTime;
use uuid::Uuid;

const CA_CERTIFICATE_FILE: &str = "ca.pem";
const CA_PRIVATE_KEY_FILE: &str = "ca-key.pem";
const REVOCATION_FILE: &str = "revoked-serials";

pub struct LocalGatewayCertificateAuthority {
    root: PathBuf,
    certificate: Certificate,
    certificate_pem: String,
    private_key: KeyPair,
    mutation_lock: Mutex<()>,
}

impl LocalGatewayCertificateAuthority {
    pub fn load_or_create(
        root: impl Into<PathBuf>,
    ) -> Result<Self, GatewayCertificateAuthorityError> {
        let root = root.into();
        ensure_private_directory(&root)?;
        let certificate_path = root.join(CA_CERTIFICATE_FILE);
        let key_path = root.join(CA_PRIVATE_KEY_FILE);
        let (certificate_pem, private_key) = match (certificate_path.exists(), key_path.exists()) {
            (true, true) => {
                require_regular_file(&certificate_path, "local Gateway CA certificate")?;
                require_regular_file(&key_path, "local Gateway CA private key")?;
                let certificate_pem = fs::read_to_string(&certificate_path)
                    .map_err(unavailable("read local Gateway CA certificate"))?;
                let key_pem = fs::read_to_string(&key_path)
                    .map_err(unavailable("read local Gateway CA private key"))?;
                let private_key = KeyPair::from_pem(&key_pem).map_err(|error| {
                    GatewayCertificateAuthorityError::Rejected(format!(
                        "local Gateway CA private key is invalid: {error}"
                    ))
                })?;
                (certificate_pem, private_key)
            }
            (false, false) => create_ca_material(&root)?,
            _ => {
                return Err(GatewayCertificateAuthorityError::Rejected(
                    "local Gateway CA certificate and private key must both exist".into(),
                ))
            }
        };
        let params = CertificateParams::from_ca_cert_pem(&certificate_pem).map_err(|error| {
            GatewayCertificateAuthorityError::Rejected(format!(
                "local Gateway CA certificate is invalid: {error}"
            ))
        })?;
        let certificate = params.self_signed(&private_key).map_err(|error| {
            GatewayCertificateAuthorityError::Rejected(format!(
                "local Gateway CA certificate and key do not form a signing identity: {error}"
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
}

#[async_trait]
impl IGatewayCertificateAuthority for LocalGatewayCertificateAuthority {
    async fn issue(
        &self,
        request: GatewayCertificateIssueRequest,
    ) -> Result<GatewayCertificateMaterial, GatewayCertificateAuthorityError> {
        request
            .validate()
            .map_err(GatewayCertificateAuthorityError::InvalidRequest)?;
        let mut csr =
            CertificateSigningRequestParams::from_pem(&request.csr_pem).map_err(|error| {
                GatewayCertificateAuthorityError::InvalidRequest(format!(
                    "Gateway certificate signing request is invalid: {error}"
                ))
            })?;
        let serial = SerialNumber::from_slice(request.certificate_id.as_uuid().as_bytes());
        csr.params.not_before = timestamp(request.issued_at.timestamp())?;
        csr.params.not_after = timestamp(request.expires_at.timestamp())?;
        csr.params.serial_number = Some(serial.clone());
        csr.params.is_ca = IsCa::NoCa;
        csr.params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        csr.params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        csr.params.subject_alt_names = request
            .dns_names
            .iter()
            .map(|dns_name| {
                dns_name
                    .as_str()
                    .try_into()
                    .map(SanType::DnsName)
                    .map_err(|error| {
                        GatewayCertificateAuthorityError::InvalidRequest(format!(
                            "Gateway certificate DNS name is invalid: {error}"
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::OrganizationName, "A3S Cloud");
        distinguished_name.push(
            DnType::CommonName,
            format!("a3s-gateway:{}", request.certificate_id),
        );
        csr.params.distinguished_name = distinguished_name;
        let certificate = csr
            .signed_by(&self.certificate, &self.private_key)
            .map_err(|error| GatewayCertificateAuthorityError::Rejected(error.to_string()))?;
        let material = GatewayCertificateMaterial {
            serial_number: serial.to_string(),
            fingerprint: format!("sha256:{:x}", Sha256::digest(certificate.der())),
            certificate_pem: certificate.pem(),
            ca_bundle_pem: self.certificate_pem.clone(),
            issued_at: request.issued_at,
            expires_at: request.expires_at,
        };
        material
            .validate()
            .map_err(GatewayCertificateAuthorityError::InvalidRequest)?;
        Ok(material)
    }

    async fn revoke(
        &self,
        certificate: &GatewayCertificate,
    ) -> Result<(), GatewayCertificateAuthorityError> {
        let serial_number = certificate
            .material
            .as_ref()
            .map(|material| material.serial_number.as_str())
            .ok_or_else(|| {
                GatewayCertificateAuthorityError::InvalidRequest(
                    "Gateway certificate has no issued material".into(),
                )
            })?;
        let _guard = self.mutation_lock.lock().map_err(|_| {
            GatewayCertificateAuthorityError::Unavailable(
                "local Gateway CA lock is poisoned".into(),
            )
        })?;
        let path = self.root.join(REVOCATION_FILE);
        require_regular_file_if_present(&path, "local Gateway CA revocation file")?;
        let existing = match fs::read_to_string(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(unavailable("read local Gateway CA revocations")(error)),
        };
        if existing.lines().any(|line| line == serial_number) {
            return Ok(());
        }
        let mut file = private_append(&path)?;
        writeln!(file, "{serial_number}")
            .map_err(unavailable("record local Gateway CA revocation"))?;
        file.sync_all()
            .map_err(unavailable("sync local Gateway CA revocation"))
    }

    async fn health(&self) -> Result<bool, GatewayCertificateAuthorityError> {
        Ok(self.root.join(CA_CERTIFICATE_FILE).is_file()
            && self.root.join(CA_PRIVATE_KEY_FILE).is_file())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableGatewayCertificateAuthority;

#[async_trait]
impl IGatewayCertificateAuthority for UnavailableGatewayCertificateAuthority {
    async fn issue(
        &self,
        _request: GatewayCertificateIssueRequest,
    ) -> Result<GatewayCertificateMaterial, GatewayCertificateAuthorityError> {
        Err(unavailable_provider())
    }

    async fn revoke(
        &self,
        _certificate: &GatewayCertificate,
    ) -> Result<(), GatewayCertificateAuthorityError> {
        Err(unavailable_provider())
    }

    async fn health(&self) -> Result<bool, GatewayCertificateAuthorityError> {
        Err(unavailable_provider())
    }
}

fn unavailable_provider() -> GatewayCertificateAuthorityError {
    GatewayCertificateAuthorityError::Unavailable(
        "a production Gateway certificate provider is not configured".into(),
    )
}

fn create_ca_material(root: &Path) -> Result<(String, KeyPair), GatewayCertificateAuthorityError> {
    let private_key = KeyPair::generate().map_err(|error| {
        GatewayCertificateAuthorityError::Unavailable(format!(
            "could not generate local Gateway CA key: {error}"
        ))
    })?;
    let mut params = CertificateParams::default();
    let now = OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::minutes(5);
    params.not_after = now + time::Duration::days(3650);
    params.serial_number = Some(SerialNumber::from_slice(Uuid::now_v7().as_bytes()));
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];
    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::OrganizationName, "A3S Cloud Development");
    distinguished_name.push(DnType::CommonName, "A3S Cloud Local Gateway CA");
    params.distinguished_name = distinguished_name;
    let certificate = params.self_signed(&private_key).map_err(|error| {
        GatewayCertificateAuthorityError::Unavailable(format!(
            "could not generate local Gateway CA certificate: {error}"
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

fn timestamp(value: i64) -> Result<OffsetDateTime, GatewayCertificateAuthorityError> {
    OffsetDateTime::from_unix_timestamp(value).map_err(|error| {
        GatewayCertificateAuthorityError::InvalidRequest(format!(
            "Gateway certificate timestamp is out of range: {error}"
        ))
    })
}

fn ensure_private_directory(path: &Path) -> Result<(), GatewayCertificateAuthorityError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(GatewayCertificateAuthorityError::Rejected(format!(
                "local Gateway CA path {} is not a real directory",
                path.display()
            )))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(unavailable("create local Gateway CA directory"))?;
        }
        Err(error) => return Err(unavailable("inspect local Gateway CA directory")(error)),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(unavailable("secure local Gateway CA directory"))?;
    }
    Ok(())
}

fn require_regular_file(path: &Path, label: &str) -> Result<(), GatewayCertificateAuthorityError> {
    let metadata =
        fs::symlink_metadata(path).map_err(unavailable("inspect local Gateway CA file"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(GatewayCertificateAuthorityError::Rejected(format!(
            "{label} is not a regular file"
        )));
    }
    Ok(())
}

fn require_regular_file_if_present(
    path: &Path,
    label: &str,
) -> Result<(), GatewayCertificateAuthorityError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
            GatewayCertificateAuthorityError::Rejected(format!("{label} is not a regular file")),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(unavailable("inspect local Gateway CA file")(error)),
    }
}

fn private_write(path: &Path, value: &str) -> Result<(), GatewayCertificateAuthorityError> {
    write_new(path, value, true)
}

fn public_write(path: &Path, value: &str) -> Result<(), GatewayCertificateAuthorityError> {
    write_new(path, value, false)
}

fn write_new(
    path: &Path,
    value: &str,
    private: bool,
) -> Result<(), GatewayCertificateAuthorityError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if private { 0o600 } else { 0o644 });
    }
    let mut file = options
        .open(path)
        .map_err(unavailable("create local Gateway CA file"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(if private {
            0o600
        } else {
            0o644
        }))
        .map_err(unavailable("secure local Gateway CA file"))?;
    }
    file.write_all(value.as_bytes())
        .map_err(unavailable("write local Gateway CA file"))?;
    file.sync_all()
        .map_err(unavailable("sync local Gateway CA file"))
}

fn private_append(path: &Path) -> Result<std::fs::File, GatewayCertificateAuthorityError> {
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(unavailable("open local Gateway CA file"))
}

fn unavailable(
    action: &'static str,
) -> impl FnOnce(std::io::Error) -> GatewayCertificateAuthorityError {
    move |error| GatewayCertificateAuthorityError::Unavailable(format!("{action}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{GatewayCertificateId, NodeId};
    use chrono::{Duration, Utc};
    use rcgen::{CertificateParams, KeyPair};
    use rustls::pki_types::CertificateDer;
    use std::io::BufReader;

    #[tokio::test]
    async fn signs_only_the_server_san_set_and_reuses_a_separate_gateway_ca() {
        let directory = tempfile::tempdir().expect("Gateway CA directory");
        let authority =
            LocalGatewayCertificateAuthority::load_or_create(directory.path()).expect("Gateway CA");
        let key = KeyPair::generate().expect("Gateway key");
        let csr = CertificateParams::new(vec!["attacker.example.com".into()])
            .expect("CSR parameters")
            .serialize_request(&key)
            .expect("CSR")
            .pem()
            .expect("CSR PEM");
        let now = Utc::now();
        let request = GatewayCertificateIssueRequest {
            certificate_id: GatewayCertificateId::new(),
            node_id: NodeId::new(),
            dns_names: vec!["*.example.com".into(), "api.example.net".into()],
            csr_pem: csr,
            issued_at: now,
            expires_at: now + Duration::days(30),
        };
        let material = authority.issue(request).await.expect("issued certificate");
        assert!(!material.certificate_pem.contains("PRIVATE KEY"));
        let certificate =
            rustls_pemfile::certs(&mut BufReader::new(material.certificate_pem.as_bytes()))
                .next()
                .expect("certificate PEM entry")
                .expect("certificate DER");
        let params = CertificateParams::from_ca_cert_der(&CertificateDer::from(
            certificate.as_ref().to_vec(),
        ))
        .expect("parse certificate");
        assert_eq!(
            params.subject_alt_names,
            vec![
                SanType::DnsName("*.example.com".try_into().expect("wildcard DNS")),
                SanType::DnsName("api.example.net".try_into().expect("exact DNS")),
            ]
        );
        assert_eq!(
            params.extended_key_usages,
            vec![ExtendedKeyUsagePurpose::ServerAuth]
        );

        let reopened =
            LocalGatewayCertificateAuthority::load_or_create(directory.path()).expect("reopen CA");
        assert_eq!(reopened.certificate_pem, material.ca_bundle_pem);
    }

    #[tokio::test]
    async fn production_provider_fails_closed() {
        let error = UnavailableGatewayCertificateAuthority
            .health()
            .await
            .expect_err("production provider must be unavailable");
        assert!(matches!(
            error,
            GatewayCertificateAuthorityError::Unavailable(_)
        ));
    }
}

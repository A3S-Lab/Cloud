use a3s_cloud_control_plane::modules::identity::domain::services::{
    IWorkloadIdentityProviderService, WorkloadIdentityProviderError,
};
use a3s_cloud_control_plane::modules::identity::domain::value_objects::{
    TrustDomainContract, TrustDomainContractSpec, TrustDomainName, WorkloadIdentityFormat,
    WorkloadIdentityProviderProfile, WorkloadIdentityProviderProfileSpec,
    WorkloadIdentityRevocationMode,
};
use a3s_cloud_control_plane::modules::identity::infrastructure::{
    SpiffeHttpsWebWorkloadIdentityProviderOptions, SpiffeHttpsWebWorkloadIdentityProviderService,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    InstallationId, Sha256Digest, TrustDomainId,
};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use rcgen::{
    generate_simple_self_signed, BasicConstraints, CertificateParams, CertifiedKey, IsCa, KeyPair,
    KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

#[tokio::test]
#[ignore = "release gate starts a real local TLS provider fixture"]
async fn real_tls_spiffe_https_web_provider_is_exact_bounded_and_drift_safe(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = BundleFixture::start().await?;
    let attestation_profile = digest('c')?;
    let profile =
        WorkloadIdentityProviderProfile::from_spec(WorkloadIdentityProviderProfileSpec {
            trust_domain: TrustDomainName::parse("cluster.example.test")?,
            bundle_endpoint_url: fixture.endpoint.clone(),
            tls_trust_anchor_digest: Some(fixture.ca_digest.clone()),
            node_attestation_profile_digests: vec![attestation_profile.clone()],
            max_credential_lifetime_seconds: 900,
            supports_revocation_epochs: false,
        })?;
    let profile_digest = profile.digest().clone();
    let options = SpiffeHttpsWebWorkloadIdentityProviderOptions {
        profile,
        tls_ca_bundle_file: fixture.ca_file.clone(),
        connect_timeout: std::time::Duration::from_secs(5),
        request_timeout: std::time::Duration::from_secs(5),
        max_bundle_bytes: 64 * 1024,
    };
    let provider = SpiffeHttpsWebWorkloadIdentityProviderService::new(&[options.clone()])?;

    assert!(matches!(
        provider.inspect(&digest('f')?).await,
        Err(WorkloadIdentityProviderError::NotConfigured(_))
    ));

    let first = provider.inspect(&profile_digest).await?;
    assert_eq!(
        first.observed_identity_formats,
        vec![
            WorkloadIdentityFormat::X509Svid,
            WorkloadIdentityFormat::JwtSvid
        ]
    );
    assert_eq!(first.trust_domain_name.as_str(), "cluster.example.test");
    assert_eq!(
        first.declared_node_attestation_profile_digests,
        vec![attestation_profile.clone()]
    );

    let contract = TrustDomainContract::from_spec(TrustDomainContractSpec {
        installation_id: InstallationId::new(),
        trust_domain_id: TrustDomainId::new(),
        name: TrustDomainName::parse("cluster.example.test")?,
        provider_profile_digest: profile_digest.clone(),
        trust_bundle_digest: first.observed_trust_bundle_digest.clone(),
        node_attestation_profile_digests: vec![attestation_profile],
        identity_formats: first.observed_identity_formats.clone(),
        max_credential_lifetime_seconds: 600,
        rotation_overlap_seconds: 60,
        revocation_mode: WorkloadIdentityRevocationMode::Expiry,
        federation_bundle_digests: vec![],
    })?;
    first.admits(&contract)?;

    let second = provider.inspect(&profile_digest).await?;
    assert_ne!(
        second.observed_trust_bundle_digest,
        first.observed_trust_bundle_digest
    );
    assert!(second.admits(&contract).is_err());

    let mismatched_profile =
        WorkloadIdentityProviderProfile::from_spec(WorkloadIdentityProviderProfileSpec {
            trust_domain: TrustDomainName::parse("cluster.example.test")?,
            bundle_endpoint_url: fixture.endpoint.clone(),
            tls_trust_anchor_digest: Some(digest('d')?),
            node_attestation_profile_digests: vec![digest('c')?],
            max_credential_lifetime_seconds: 900,
            supports_revocation_epochs: false,
        })?;
    let mismatch = SpiffeHttpsWebWorkloadIdentityProviderOptions {
        profile: mismatched_profile,
        ..options
    };
    assert!(SpiffeHttpsWebWorkloadIdentityProviderService::new(&[mismatch]).is_err());

    fixture.finish().await?;
    println!(
        "\nA3S_CLOUD_WI1_PROVIDER_CERTIFIED profile={} bundle={} checks=7/7",
        profile_digest.as_str(),
        first.observed_trust_bundle_digest.as_str()
    );
    Ok(())
}

struct BundleFixture {
    endpoint: String,
    ca_file: String,
    ca_digest: Sha256Digest,
    directory: TempDir,
    server: JoinHandle<Result<(), String>>,
}

impl BundleFixture {
    async fn start() -> Result<Self, Box<dyn std::error::Error>> {
        let CertifiedKey { cert, key_pair } =
            generate_simple_self_signed(vec!["127.0.0.1".into()])?;
        let mut bundle_ca_parameters = CertificateParams::new(Vec::<String>::new())?;
        bundle_ca_parameters.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        bundle_ca_parameters.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];
        let bundle_ca_key = KeyPair::generate()?;
        let bundle_ca = bundle_ca_parameters.self_signed(&bundle_ca_key)?;
        let bundle_certificate = STANDARD.encode(bundle_ca.der().as_ref());
        let bodies = vec![
            spiffe_bundle(41, "jwt-root-1", 0xa5, &bundle_certificate),
            spiffe_bundle(42, "jwt-root-2", 0xb7, &bundle_certificate),
        ];
        let server_config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert.der().to_vec())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der())),
        )?;
        let directory = tempfile::tempdir()?;
        let ca_path = directory.path().join("provider-ca.pem");
        let ca_pem = cert.pem();
        std::fs::write(&ca_path, ca_pem.as_bytes())?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let acceptor = TlsAcceptor::from(Arc::new(server_config));
        let server = tokio::spawn(async move {
            for body in bodies {
                let (stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
                let mut tls = acceptor
                    .accept(stream)
                    .await
                    .map_err(|error| error.to_string())?;
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                loop {
                    let read = tls
                        .read(&mut buffer)
                        .await
                        .map_err(|error| error.to_string())?;
                    if read == 0 {
                        return Err("provider fixture request ended before headers".into());
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                    if request.len() > 16 * 1024 {
                        return Err("provider fixture request exceeded its bound".into());
                    }
                }
                let request = std::str::from_utf8(&request)
                    .map_err(|_| "provider fixture request was not UTF-8".to_owned())?;
                if !request.starts_with("GET /bundle HTTP/1.1\r\n")
                    || !request
                        .to_ascii_lowercase()
                        .contains("accept: application/json")
                {
                    return Err("provider fixture received an unexpected request".into());
                }
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                tls.write_all(headers.as_bytes())
                    .await
                    .map_err(|error| error.to_string())?;
                tls.write_all(&body)
                    .await
                    .map_err(|error| error.to_string())?;
                tls.shutdown().await.map_err(|error| error.to_string())?;
            }
            Ok(())
        });
        Ok(Self {
            endpoint: format!("https://{address}/bundle"),
            ca_file: ca_path.to_string_lossy().into_owned(),
            ca_digest: Sha256Digest::from_bytes(ca_pem.as_bytes()),
            directory,
            server,
        })
    }

    async fn finish(self) -> Result<(), Box<dyn std::error::Error>> {
        self.server.await??;
        assert!(self.directory.path().is_dir());
        Ok(())
    }
}

fn digest(byte: char) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64)))
}

fn spiffe_bundle(sequence: u64, key_id: &str, modulus_byte: u8, certificate: &str) -> Vec<u8> {
    let modulus = URL_SAFE_NO_PAD.encode(vec![modulus_byte; 256]);
    format!(
        r#"{{
  "spiffe_sequence": {sequence},
  "spiffe_refresh_hint": 300,
  "keys": [
    {{"kty":"RSA","use":"jwt-svid","kid":"{key_id}","alg":"RS256","n":"{modulus}","e":"AQAB"}},
    {{"kty":"EC","use":"x509-svid","x5c":["{certificate}"]}}
  ]
}}"#
    )
    .into_bytes()
}

use crate::modules::identity::domain::services::{
    IWorkloadIdentityProviderService, WorkloadIdentityProviderError,
    WorkloadIdentityProviderInspection,
};
use crate::modules::identity::domain::value_objects::{
    WorkloadIdentityFormat, WorkloadIdentityProviderProfile,
};
use crate::modules::shared_kernel::domain::{canonical_json_bounded, Sha256Digest};
use async_trait::async_trait;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use futures_util::StreamExt;
use reqwest::header::ACCEPT;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Number, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use x509_parser::parse_x509_certificate;

const MAX_TLS_CA_BUNDLE_BYTES: u64 = 1024 * 1024;
const MAX_BUNDLE_KEYS: usize = 512;
const MAX_CANONICAL_JWK_BYTES: usize = 512 * 1024;
const MIN_RSA_MODULUS_BYTES: usize = 256;
const MAX_RSA_MODULUS_BYTES: usize = 1024;
const MAX_JWK_KEY_COMPONENT_BYTES: usize = 2048;
const MAX_CONCURRENT_INSPECTIONS_PER_PROVIDER: usize = 4;
const MAX_PROVIDER_TIMEOUT: Duration = Duration::from_secs(60);
const MIN_PROVIDER_BUNDLE_BYTES: usize = 1024;
const MAX_PROVIDER_BUNDLE_BYTES: usize = 1024 * 1024;
const PRIVATE_JWK_MEMBERS: [&str; 8] = ["d", "p", "q", "dp", "dq", "qi", "oth", "k"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpiffeHttpsWebWorkloadIdentityProviderOptions {
    pub profile: WorkloadIdentityProviderProfile,
    pub tls_ca_bundle_file: String,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub max_bundle_bytes: usize,
}

impl SpiffeHttpsWebWorkloadIdentityProviderOptions {
    pub fn validate(&self) -> Result<(), String> {
        self.profile.validate()?;
        if self.profile.spec().supports_revocation_epochs {
            return Err(
                "SPIFFE https_web provider cannot advertise revocation epochs before WI6".into(),
            );
        }
        if self.profile.spec().tls_trust_anchor_digest.is_some()
            == self.tls_ca_bundle_file.is_empty()
            || !valid_optional_ca_bundle_file_path(&self.tls_ca_bundle_file)
            || self.connect_timeout.is_zero()
            || self.connect_timeout > MAX_PROVIDER_TIMEOUT
            || self.request_timeout.is_zero()
            || self.request_timeout > MAX_PROVIDER_TIMEOUT
            || self.connect_timeout > self.request_timeout
            || !(MIN_PROVIDER_BUNDLE_BYTES..=MAX_PROVIDER_BUNDLE_BYTES)
                .contains(&self.max_bundle_bytes)
        {
            return Err("workload identity provider operational bounds are invalid".into());
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct SpiffeHttpsWebWorkloadIdentityProviderService {
    providers: Arc<BTreeMap<Sha256Digest, BoundProvider>>,
}

#[derive(Clone)]
struct BoundProvider {
    profile: WorkloadIdentityProviderProfile,
    client: reqwest::Client,
    max_bundle_bytes: usize,
    inspection_permits: Arc<Semaphore>,
}

impl SpiffeHttpsWebWorkloadIdentityProviderService {
    pub fn new(options: &[SpiffeHttpsWebWorkloadIdentityProviderOptions]) -> Result<Self, String> {
        let providers = options
            .iter()
            .map(|options| {
                let provider = BoundProvider::new(options)?;
                Ok((provider.profile.digest().clone(), provider))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        if providers.len() != options.len() {
            return Err("workload identity provider profile digests must be unique".into());
        }
        Ok(Self {
            providers: Arc::new(providers),
        })
    }
}

impl BoundProvider {
    fn new(options: &SpiffeHttpsWebWorkloadIdentityProviderOptions) -> Result<Self, String> {
        options.validate()?;

        let mut builder = reqwest::Client::builder()
            .use_rustls_tls()
            .https_only(true)
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .connect_timeout(options.connect_timeout)
            .timeout(options.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .no_proxy()
            .user_agent("a3s-cloud-workload-identity/1");
        if let Some(expected_digest) = &options.profile.spec().tls_trust_anchor_digest {
            let pem = read_pinned_ca_bundle(&options.tls_ca_bundle_file, expected_digest)?;
            let certificates = reqwest::Certificate::from_pem_bundle(&pem)
                .map_err(|_| "workload identity provider TLS CA bundle is invalid".to_owned())?;
            if certificates.is_empty() {
                return Err(
                    "workload identity provider TLS CA bundle contains no certificate".into(),
                );
            }
            builder = builder.tls_built_in_root_certs(false);
            for certificate in certificates {
                builder = builder.add_root_certificate(certificate);
            }
        } else if !options.tls_ca_bundle_file.is_empty() {
            return Err(
                "workload identity provider TLS CA file requires an exact profile digest".into(),
            );
        }
        let client = builder
            .build()
            .map_err(|_| "workload identity provider HTTP client could not be built".to_owned())?;
        Ok(Self {
            profile: options.profile.clone(),
            client,
            max_bundle_bytes: options.max_bundle_bytes,
            inspection_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_INSPECTIONS_PER_PROVIDER)),
        })
    }

    async fn inspect(
        &self,
    ) -> Result<WorkloadIdentityProviderInspection, WorkloadIdentityProviderError> {
        let _permit = Arc::clone(&self.inspection_permits)
            .try_acquire_owned()
            .map_err(|_| unavailable())?;
        let endpoint = &self.profile.spec().bundle_endpoint_url;
        let response = self
            .client
            .get(endpoint)
            .header(ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| unavailable())?;
        if response.url().as_str() != endpoint {
            return Err(WorkloadIdentityProviderError::Rejected(
                "bundle endpoint redirected away from its pinned URL".into(),
            ));
        }
        let status = response.status();
        if !status.is_success() {
            return Err(if status.is_server_error() {
                unavailable()
            } else {
                WorkloadIdentityProviderError::Rejected(format!(
                    "bundle endpoint returned HTTP {}",
                    status.as_u16()
                ))
            });
        }
        if response
            .content_length()
            .is_some_and(|length| length > self.max_bundle_bytes as u64)
        {
            return Err(invalid_observation(
                "bundle response exceeded its byte bound",
            ));
        }
        let mut body = Vec::new();
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(|_| unavailable())?;
            if body
                .len()
                .checked_add(chunk.len())
                .is_none_or(|length| length > self.max_bundle_bytes)
            {
                return Err(invalid_observation(
                    "bundle response exceeded its byte bound",
                ));
            }
            body.extend_from_slice(&chunk);
        }
        let (canonical_bundle, identity_formats) = inspect_bundle(&body, self.max_bundle_bytes)?;
        let inspection = WorkloadIdentityProviderInspection {
            provider_profile_digest: self.profile.digest().clone(),
            trust_domain_name: self.profile.spec().trust_domain.clone(),
            observed_trust_bundle_digest: Sha256Digest::from_bytes(&canonical_bundle),
            observed_federation_bundle_digests: vec![],
            observed_identity_formats: identity_formats,
            declared_node_attestation_profile_digests: self
                .profile
                .spec()
                .node_attestation_profile_digests
                .clone(),
            declared_max_credential_lifetime_seconds: self
                .profile
                .spec()
                .max_credential_lifetime_seconds,
            declared_supports_revocation_epochs: false,
        };
        inspection
            .validate()
            .map_err(|_| invalid_observation("provider inspection was invalid"))?;
        Ok(inspection)
    }
}

#[async_trait]
impl IWorkloadIdentityProviderService for SpiffeHttpsWebWorkloadIdentityProviderService {
    async fn inspect(
        &self,
        provider_profile_digest: &Sha256Digest,
    ) -> Result<WorkloadIdentityProviderInspection, WorkloadIdentityProviderError> {
        let provider = self.providers.get(provider_profile_digest).ok_or_else(|| {
            WorkloadIdentityProviderError::NotConfigured(
                "provider profile digest is not configured".into(),
            )
        })?;
        provider.inspect().await
    }
}

fn read_pinned_ca_bundle(path: &str, expected: &Sha256Digest) -> Result<Vec<u8>, String> {
    if path.is_empty() || !valid_optional_ca_bundle_file_path(path) {
        return Err("workload identity provider TLS CA bundle path is invalid".into());
    }
    let metadata = std::fs::metadata(path)
        .map_err(|_| "workload identity provider TLS CA bundle is unavailable".to_owned())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_TLS_CA_BUNDLE_BYTES {
        return Err("workload identity provider TLS CA bundle size is invalid".into());
    }
    let pem = std::fs::read(path)
        .map_err(|_| "workload identity provider TLS CA bundle is unavailable".to_owned())?;
    if pem.is_empty()
        || pem.len() as u64 > MAX_TLS_CA_BUNDLE_BYTES
        || &Sha256Digest::from_bytes(&pem) != expected
    {
        return Err("workload identity provider TLS CA bundle digest does not match".into());
    }
    Ok(pem)
}

fn valid_optional_ca_bundle_file_path(value: &str) -> bool {
    value.is_empty()
        || (value.len() <= 4096
            && value.trim() == value
            && !value.contains(['\0', '\r', '\n'])
            && !std::path::Path::new(value)
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir)))
}

fn inspect_bundle(
    body: &[u8],
    maximum_bytes: usize,
) -> Result<(Vec<u8>, Vec<WorkloadIdentityFormat>), WorkloadIdentityProviderError> {
    if body.is_empty() || body.len() > maximum_bytes {
        return Err(invalid_observation("bundle body size was invalid"));
    }
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let StrictJsonValue(mut bundle) = StrictJsonValue::deserialize(&mut deserializer)
        .map_err(|_| invalid_observation("bundle was not strict UTF-8 JSON"))?;
    deserializer
        .end()
        .map_err(|_| invalid_observation("bundle contained trailing JSON input"))?;
    let root = bundle
        .as_object_mut()
        .ok_or_else(|| invalid_observation("bundle root was not an object"))?;
    validate_optional_u64(root, "spiffe_sequence", false)?;
    validate_optional_u64(root, "spiffe_refresh_hint", true)?;
    let keys = root
        .get_mut("keys")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid_observation("bundle keys member was not an array"))?;
    if keys.is_empty() || keys.len() > MAX_BUNDLE_KEYS {
        return Err(invalid_observation("bundle key count was outside bounds"));
    }

    let mut formats = BTreeSet::new();
    let mut jwt_key_ids = BTreeSet::new();
    let mut canonical_keys = Vec::with_capacity(keys.len());
    for key in keys.drain(..) {
        inspect_jwk(&key, &mut formats, &mut jwt_key_ids)?;
        let canonical = canonical_json_bounded(&key, MAX_CANONICAL_JWK_BYTES, "SPIFFE JWK")
            .map_err(|_| invalid_observation("bundle JWK exceeded canonical bounds"))?;
        canonical_keys.push((canonical, key));
    }
    canonical_keys.sort_by(|left, right| left.0.cmp(&right.0));
    if canonical_keys.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(invalid_observation("bundle contained duplicate JWKs"));
    }
    *keys = canonical_keys.into_iter().map(|(_, key)| key).collect();
    let identity_formats = formats.into_iter().collect::<Vec<_>>();
    if identity_formats.is_empty() {
        return Err(invalid_observation(
            "bundle contained no supported SPIFFE verification key",
        ));
    }
    let canonical = canonical_json_bounded(&bundle, maximum_bytes, "SPIFFE trust bundle")
        .map_err(|_| invalid_observation("bundle exceeded canonical bounds"))?;
    Ok((canonical, identity_formats))
}

fn validate_optional_u64(
    root: &Map<String, Value>,
    name: &str,
    require_positive: bool,
) -> Result<(), WorkloadIdentityProviderError> {
    let Some(value) = root.get(name) else {
        return Ok(());
    };
    let value = value
        .as_u64()
        .ok_or_else(|| invalid_observation("bundle metadata was not an unsigned integer"))?;
    if require_positive && value == 0 {
        return Err(invalid_observation("bundle refresh hint was zero"));
    }
    Ok(())
}

fn inspect_jwk(
    key: &Value,
    formats: &mut BTreeSet<WorkloadIdentityFormat>,
    jwt_key_ids: &mut BTreeSet<String>,
) -> Result<(), WorkloadIdentityProviderError> {
    let key = key
        .as_object()
        .ok_or_else(|| invalid_observation("bundle JWK was not an object"))?;
    if PRIVATE_JWK_MEMBERS
        .iter()
        .any(|member| key.contains_key(*member))
    {
        return Err(invalid_observation(
            "bundle JWK exposed private or symmetric key material",
        ));
    }
    let Some(use_) = key.get("use").and_then(Value::as_str) else {
        return Ok(());
    };
    let format = match use_ {
        "x509-svid" => WorkloadIdentityFormat::X509Svid,
        "jwt-svid" => WorkloadIdentityFormat::JwtSvid,
        _ => return Ok(()),
    };
    let Some(key_type) = key
        .get("kty")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 32 && value.is_ascii())
    else {
        return Ok(());
    };
    match format {
        WorkloadIdentityFormat::X509Svid => {
            if !matches!(key_type, "RSA" | "EC" | "OKP") {
                return Ok(());
            }
            validate_x509_svid_jwk(key)?;
        }
        WorkloadIdentityFormat::JwtSvid => {
            if !matches!(key_type, "RSA" | "EC") {
                return Ok(());
            }
            let key_id = validate_jwt_svid_jwk(key, key_type)?;
            if !jwt_key_ids.insert(key_id.to_owned()) {
                return Err(invalid_observation(
                    "bundle contained duplicate JWT-SVID key IDs",
                ));
            }
        }
    }
    formats.insert(format);
    Ok(())
}

fn validate_x509_svid_jwk(key: &Map<String, Value>) -> Result<(), WorkloadIdentityProviderError> {
    if key.contains_key("kid") {
        return Err(invalid_observation("X509-SVID JWK contained a key ID"));
    }
    let certificates = key
        .get("x5c")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_observation("X509-SVID JWK had no certificate"))?;
    if certificates.len() != 1 {
        return Err(invalid_observation(
            "X509-SVID JWK did not contain exactly one CA certificate",
        ));
    }
    let encoded = certificates[0]
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= MAX_CANONICAL_JWK_BYTES)
        .ok_or_else(|| invalid_observation("X509-SVID JWK certificate was not bounded base64"))?;
    let der = STANDARD
        .decode(encoded)
        .map_err(|_| invalid_observation("X509-SVID JWK certificate was not base64 DER"))?;
    let (remaining, certificate) = parse_x509_certificate(&der)
        .map_err(|_| invalid_observation("X509-SVID JWK certificate was not valid DER X.509"))?;
    if !remaining.is_empty() || certificate.tbs_certificate.extensions_map().is_err() {
        return Err(invalid_observation(
            "X509-SVID JWK certificate contained trailing data or duplicate extensions",
        ));
    }
    let basic_constraints = certificate
        .tbs_certificate
        .basic_constraints()
        .map_err(|_| invalid_observation("X509-SVID JWK CA constraints were invalid"))?
        .ok_or_else(|| invalid_observation("X509-SVID JWK had no CA constraints"))?;
    let key_usage = certificate
        .tbs_certificate
        .key_usage()
        .map_err(|_| invalid_observation("X509-SVID JWK key usage was invalid"))?
        .ok_or_else(|| invalid_observation("X509-SVID JWK had no key usage"))?;
    if !basic_constraints.critical
        || !basic_constraints.value.ca
        || !key_usage.critical
        || !key_usage.value.key_cert_sign()
    {
        return Err(invalid_observation(
            "X509-SVID JWK certificate was not an admitted CA signer",
        ));
    }
    Ok(())
}

fn validate_jwt_svid_jwk<'a>(
    key: &'a Map<String, Value>,
    key_type: &str,
) -> Result<&'a str, WorkloadIdentityProviderError> {
    let key_id = key
        .get("kid")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 1024 && value.is_ascii())
        .ok_or_else(|| invalid_observation("JWT-SVID JWK had no bounded key ID"))?;
    match key_type {
        "RSA" => {
            let modulus = decode_base64url_member(key, "n")?;
            let exponent = decode_base64url_member(key, "e")?;
            let exponent_value = exponent
                .iter()
                .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte));
            if !(MIN_RSA_MODULUS_BYTES..=MAX_RSA_MODULUS_BYTES).contains(&modulus.len())
                || modulus.first() == Some(&0)
                || modulus.last().is_some_and(|byte| byte & 1 == 0)
                || exponent.is_empty()
                || exponent.len() > 8
                || exponent.first() == Some(&0)
                || exponent_value.is_multiple_of(2)
                || exponent_value < 3
            {
                return Err(invalid_observation(
                    "JWT-SVID RSA JWK key material was outside bounds",
                ));
            }
            validate_optional_jwt_algorithm(
                key,
                &["RS256", "RS384", "RS512", "PS256", "PS384", "PS512"],
            )?;
        }
        "EC" => {
            let curve = key
                .get("crv")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_observation("JWT-SVID EC JWK had no curve"))?;
            let (coordinate_bytes, algorithm) = match curve {
                "P-256" => (32, "ES256"),
                "P-384" => (48, "ES384"),
                "P-521" => (66, "ES512"),
                _ => {
                    return Err(invalid_observation(
                        "JWT-SVID EC JWK used an unsupported curve",
                    ))
                }
            };
            if decode_base64url_member(key, "x")?.len() != coordinate_bytes
                || decode_base64url_member(key, "y")?.len() != coordinate_bytes
            {
                return Err(invalid_observation(
                    "JWT-SVID EC JWK coordinates were outside bounds",
                ));
            }
            validate_optional_jwt_algorithm(key, &[algorithm])?;
        }
        _ => unreachable!("unsupported JWT key types are ignored before validation"),
    }
    Ok(key_id)
}

fn decode_base64url_member(
    key: &Map<String, Value>,
    name: &str,
) -> Result<Vec<u8>, WorkloadIdentityProviderError> {
    let encoded = key
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= MAX_JWK_KEY_COMPONENT_BYTES)
        .ok_or_else(|| invalid_observation("JWT-SVID JWK key material was missing"))?;
    URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| invalid_observation("JWT-SVID JWK key material was not base64url"))
}

fn validate_optional_jwt_algorithm(
    key: &Map<String, Value>,
    allowed: &[&str],
) -> Result<(), WorkloadIdentityProviderError> {
    match key.get("alg") {
        None => Ok(()),
        Some(Value::String(value)) if allowed.contains(&value.as_str()) => Ok(()),
        Some(_) => Err(invalid_observation(
            "JWT-SVID JWK algorithm did not match its public key",
        )),
    }
}

fn unavailable() -> WorkloadIdentityProviderError {
    WorkloadIdentityProviderError::Unavailable(
        "configured https_web bundle endpoint could not be reached".into(),
    )
}

fn invalid_observation(message: &str) -> WorkloadIdentityProviderError {
    WorkloadIdentityProviderError::InvalidObservation(message.into())
}

struct StrictJsonValue(Value);

impl<'de> Deserialize<'de> for StrictJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictJsonVisitor)
    }
}

struct StrictJsonVisitor;

impl<'de> Visitor<'de> for StrictJsonVisitor {
    type Value = StrictJsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object members")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Number::from_f64(value)
            .map(|number| StrictJsonValue(Value::Number(number)))
            .ok_or_else(|| E::custom("JSON number is not finite"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_string(value.to_owned())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictJsonValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictJsonValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(StrictJsonValue(value)) = sequence.next_element::<StrictJsonValue>()? {
            values.push(value);
        }
        Ok(StrictJsonValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut entries: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some(key) = entries.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom("JSON object member is duplicated"));
            }
            let StrictJsonValue(value) = entries.next_value::<StrictJsonValue>()?;
            values.insert(key, value);
        }
        Ok(StrictJsonValue(Value::Object(values)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::{
        TrustDomainName, WorkloadIdentityProviderProfileSpec,
    };
    use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair, KeyUsagePurpose};

    fn ca_certificate() -> String {
        let mut parameters = CertificateParams::new(Vec::<String>::new()).expect("parameters");
        parameters.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        parameters.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];
        let key = KeyPair::generate().expect("key");
        let certificate = parameters.self_signed(&key).expect("CA certificate");
        STANDARD.encode(certificate.der().as_ref())
    }

    fn rsa_modulus(byte: u8) -> String {
        URL_SAFE_NO_PAD.encode(vec![byte; MIN_RSA_MODULUS_BYTES])
    }

    fn provider_options(
        tls_trust_anchor_digest: Option<Sha256Digest>,
        supports_revocation_epochs: bool,
    ) -> SpiffeHttpsWebWorkloadIdentityProviderOptions {
        SpiffeHttpsWebWorkloadIdentityProviderOptions {
            profile: WorkloadIdentityProviderProfile::from_spec(
                WorkloadIdentityProviderProfileSpec {
                    trust_domain: TrustDomainName::parse("cluster.example.test")
                        .expect("trust domain"),
                    bundle_endpoint_url: "https://identity.example.test/bundle".into(),
                    tls_trust_anchor_digest,
                    node_attestation_profile_digests: vec![Sha256Digest::parse(format!(
                        "sha256:{}",
                        "a".repeat(64)
                    ))
                    .expect("attestation profile")],
                    max_credential_lifetime_seconds: 900,
                    supports_revocation_epochs,
                },
            )
            .expect("provider profile"),
            tls_ca_bundle_file: String::new(),
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(10),
            max_bundle_bytes: 64 * 1024,
        }
    }

    #[test]
    fn provider_options_keep_one_closed_operational_boundary() {
        provider_options(None, false).validate().expect("options");

        let mut zero_timeout = provider_options(None, false);
        zero_timeout.connect_timeout = Duration::ZERO;
        assert!(zero_timeout.validate().is_err());

        let mut parent_path = provider_options(
            Some(Sha256Digest::parse(format!("sha256:{}", "b".repeat(64))).expect("TLS digest")),
            false,
        );
        parent_path.tls_ca_bundle_file = "../provider-ca.pem".into();
        assert!(parent_path.validate().is_err());

        assert!(provider_options(None, true).validate().is_err());
    }

    #[test]
    fn bundle_digest_is_key_order_stable_and_formats_are_observed() {
        let certificate = ca_certificate();
        let modulus = rsa_modulus(0xa5);
        let first = format!(
            r#"{{
            "spiffe_sequence": 7,
            "keys": [
                {{"kty":"RSA","use":"jwt-svid","kid":"jwt-b","n":"{modulus}","e":"AQAB"}},
                {{"kty":"EC","use":"x509-svid","x5c":["{certificate}"]}}
            ]
        }}"#
        );
        let second = format!(
            r#"{{
            "keys": [
                {{"x5c":["{certificate}"],"use":"x509-svid","kty":"EC"}},
                {{"e":"AQAB","n":"{modulus}","kid":"jwt-b","use":"jwt-svid","kty":"RSA"}}
            ],
            "spiffe_sequence": 7
        }}"#
        );
        let (first, formats) = inspect_bundle(first.as_bytes(), 64 * 1024).expect("first bundle");
        let (second, replay_formats) =
            inspect_bundle(second.as_bytes(), 64 * 1024).expect("second bundle");
        assert_eq!(
            Sha256Digest::from_bytes(&first),
            Sha256Digest::from_bytes(&second)
        );
        assert_eq!(formats, replay_formats);
        assert_eq!(
            formats,
            vec![
                WorkloadIdentityFormat::X509Svid,
                WorkloadIdentityFormat::JwtSvid
            ]
        );
    }

    #[test]
    fn strict_bundle_parser_rejects_duplicate_or_ambiguous_inputs() {
        let modulus = rsa_modulus(0xa5);
        let certificate = ca_certificate();
        let invalid = vec![
            r#"{"keys":[],"keys":[]}"#.to_owned(),
            format!(
                r#"{{"keys":[{{"kty":"RSA","use":"jwt-svid","kid":"same","n":"{modulus}","e":"AQAB"}},{{"kty":"RSA","use":"jwt-svid","kid":"same","n":"{modulus}","e":"AQAB"}}]}}"#
            ),
            r#"{"keys":[{"kty":"RSA","use":"x509-svid","x5c":[]}]}"#.to_owned(),
            r#"{"spiffe_refresh_hint":0,"keys":[{"kty":"RSA","use":"x509-svid","x5c":["YQ=="]}]}"#
                .to_owned(),
            format!(
                r#"{{"keys":[{{"kty":"EC","use":"x509-svid","kid":"forbidden","x5c":["{certificate}"]}}]}}"#
            ),
            format!(
                r#"{{"keys":[{{"kty":"RSA","use":"jwt-svid","kid":"private","n":"{modulus}","e":"AQAB","d":"AQAB"}}]}}"#
            ),
            r#"{"keys":[{"kty":"oct","use":"extension","k":"c2VjcmV0"},{"kty":"future","use":"future"}]}"#
                .to_owned(),
        ];
        for invalid in invalid {
            assert!(inspect_bundle(invalid.as_bytes(), 64 * 1024).is_err());
        }
    }

    #[test]
    fn strict_bundle_parser_enforces_the_post_stream_byte_bound() {
        let certificate = ca_certificate();
        let bundle =
            format!(r#"{{"keys":[{{"kty":"EC","use":"x509-svid","x5c":["{certificate}"]}}]}}"#);
        assert!(inspect_bundle(bundle.as_bytes(), bundle.len() - 1).is_err());
    }
}

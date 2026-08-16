use crate::modules::shared_kernel::domain::{Sha256Digest, StorageNamespaceId};
use a3s_acl::builder::{boolean, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use serde::{Deserialize, Serialize};

const PROFILE_BLOCK: &str = "object_namespace_provider";
const PROFILE_LABEL: &str = "s3_compatible";
const PROFILE_ATTRIBUTES: [&str; 6] = [
    "bucket",
    "endpoint",
    "prefix",
    "region",
    "schema",
    "virtual_hosted_style",
];

pub const OBJECT_NAMESPACE_PROVIDER_PROFILE_SCHEMA: &str =
    "cloud.object-namespace.provider-profile.v1";
pub const OBJECT_NAMESPACE_PROVIDER_PROFILE_MAX_ACL_BYTES: usize = 16 * 1024;

/// Immutable, non-secret S0 provider semantics.
///
/// Credentials remain exact Secrets-owned references in
/// `ObjectNamespaceCredentialBinding`. This profile is the sole authority for
/// endpoint, bucket, region, addressing, and namespace-prefix semantics used
/// by an S0 provider adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectNamespaceProviderProfileSpec {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub virtual_hosted_style: bool,
}

impl ObjectNamespaceProviderProfileSpec {
    pub fn validate(&self) -> Result<(), String> {
        if self.endpoint != canonical_https_origin(&self.endpoint)? {
            return Err(
                "object namespace provider endpoint must be one canonical HTTPS origin".into(),
            );
        }
        validate_region(&self.region)?;
        validate_bucket(&self.bucket)?;
        validate_prefix(&self.prefix)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectNamespaceProviderProfile {
    spec: ObjectNamespaceProviderProfileSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl ObjectNamespaceProviderProfile {
    pub fn from_spec(mut spec: ObjectNamespaceProviderProfileSpec) -> Result<Self, String> {
        spec.endpoint = canonical_https_origin(&spec.endpoint)?;
        spec.validate()?;
        let document = profile_document(&spec);
        let canonical_acl = generate_acl(&document);
        if canonical_acl.len() > OBJECT_NAMESPACE_PROVIDER_PROFILE_MAX_ACL_BYTES {
            return Err("object namespace provider profile ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl).map_err(|error| {
            format!("generated object namespace provider profile ACL is invalid: {error}")
        })?;
        let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
            format!("object namespace provider profile is not canonicalizable: {error}")
        })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(acl: &str) -> Result<Self, String> {
        if acl.is_empty() || acl.len() > OBJECT_NAMESPACE_PROVIDER_PROFILE_MAX_ACL_BYTES {
            return Err("object namespace provider profile ACL size is invalid".into());
        }
        let document = parse_acl(acl).map_err(|error| {
            format!("object namespace provider profile ACL is invalid: {error}")
        })?;
        Self::from_spec(parse_profile(&document)?)
    }

    pub fn restore(acl: &str, stored_digest: &str) -> Result<Self, String> {
        let profile = Self::parse_acl(acl)?;
        if profile.canonical_acl != acl || profile.digest.as_str() != stored_digest {
            return Err(
                "stored object namespace provider profile ACL and digest do not match".into(),
            );
        }
        Ok(profile)
    }

    pub fn validate(&self) -> Result<(), String> {
        let restored = Self::restore(&self.canonical_acl, self.digest.as_str())?;
        if &restored != self {
            return Err("object namespace provider profile drifted from canonical ACL".into());
        }
        Ok(())
    }

    pub fn namespace_prefix(&self, namespace_id: StorageNamespaceId) -> Result<String, String> {
        self.validate()?;
        if namespace_id.as_uuid().is_nil() {
            return Err("object namespace provider scope requires a non-nil namespace ID".into());
        }
        Ok(format!("{}/{}", self.spec.prefix, namespace_id))
    }

    pub const fn spec(&self) -> &ObjectNamespaceProviderProfileSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn profile_document(spec: &ObjectNamespaceProviderProfileSpec) -> Document {
    Document {
        blocks: vec![BlockBuilder::new(PROFILE_BLOCK)
            .label(PROFILE_LABEL)
            .attr("schema", string(OBJECT_NAMESPACE_PROVIDER_PROFILE_SCHEMA))
            .attr("endpoint", string(&spec.endpoint))
            .attr("region", string(&spec.region))
            .attr("bucket", string(&spec.bucket))
            .attr("prefix", string(&spec.prefix))
            .attr("virtual_hosted_style", boolean(spec.virtual_hosted_style))
            .build()],
    }
}

fn parse_profile(document: &Document) -> Result<ObjectNamespaceProviderProfileSpec, String> {
    if document.blocks.len() != 1 {
        return Err("object namespace provider profile must contain exactly one block".into());
    }
    let block = &document.blocks[0];
    if block.name != PROFILE_BLOCK
        || block.labels.as_slice() != [PROFILE_LABEL]
        || !block.blocks.is_empty()
        || block.attributes.len() != PROFILE_ATTRIBUTES.len()
        || block
            .attributes
            .keys()
            .any(|key| !PROFILE_ATTRIBUTES.contains(&key.as_str()))
    {
        return Err("object namespace provider profile block shape is invalid".into());
    }
    require_exact_string(block, "schema", OBJECT_NAMESPACE_PROVIDER_PROFILE_SCHEMA)?;
    Ok(ObjectNamespaceProviderProfileSpec {
        endpoint: required_string(block, "endpoint")?,
        region: required_string(block, "region")?,
        bucket: required_string(block, "bucket")?,
        prefix: required_string(block, "prefix")?,
        virtual_hosted_style: required_boolean(block, "virtual_hosted_style")?,
    })
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    match block.attributes.get(name) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(format!(
            "object namespace provider profile field {name:?} must be a string"
        )),
        None => Err(format!(
            "object namespace provider profile field {name:?} is required"
        )),
    }
}

fn require_exact_string(block: &Block, name: &str, expected: &str) -> Result<(), String> {
    if required_string(block, name)? != expected {
        return Err(format!(
            "object namespace provider profile field {name:?} must be exactly {expected:?}"
        ));
    }
    Ok(())
}

fn required_boolean(block: &Block, name: &str) -> Result<bool, String> {
    match block.attributes.get(name) {
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(format!(
            "object namespace provider profile field {name:?} must be a boolean"
        )),
        None => Err(format!(
            "object namespace provider profile field {name:?} is required"
        )),
    }
}

fn canonical_https_origin(value: &str) -> Result<String, String> {
    if value.is_empty() || value.len() > 2048 || value.contains(['\0', '\r', '\n', ' ', '\t']) {
        return Err("object namespace provider endpoint is invalid".into());
    }
    let endpoint = url::Url::parse(value)
        .map_err(|_| "object namespace provider endpoint is invalid".to_owned())?;
    if endpoint.scheme() != "https"
        || endpoint.host_str().is_none()
        || endpoint.path() != "/"
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
    {
        return Err("object namespace provider endpoint must be one HTTPS origin".into());
    }
    Ok(endpoint.origin().ascii_serialization())
}

fn validate_region(value: &str) -> Result<(), String> {
    let valid_edge = value
        .as_bytes()
        .first()
        .zip(value.as_bytes().last())
        .is_some_and(|(first, last)| first.is_ascii_alphanumeric() && last.is_ascii_alphanumeric());
    if value.len() > 128
        || !valid_edge
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'))
    {
        return Err("object namespace provider region is invalid".into());
    }
    Ok(())
}

fn validate_bucket(value: &str) -> Result<(), String> {
    let valid_edge = value
        .as_bytes()
        .first()
        .zip(value.as_bytes().last())
        .is_some_and(|(first, last)| first.is_ascii_alphanumeric() && last.is_ascii_alphanumeric());
    if !(3..=63).contains(&value.len())
        || !valid_edge
        || value.bytes().any(|byte| {
            !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-'))
        })
        || value.contains("..")
        || value.contains(".-")
        || value.contains("-.")
    {
        return Err("object namespace provider bucket is invalid".into());
    }
    Ok(())
}

fn validate_prefix(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 1024
        || value.starts_with('/')
        || value.ends_with('/')
        || value.split('/').any(|segment| {
            segment.is_empty()
                || matches!(segment, "." | "..")
                || segment.len() > 255
                || segment.bytes().any(|byte| {
                    !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
                })
        })
    {
        return Err("object namespace provider prefix is invalid".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/s0.1/object-namespace-provider-profile.acl"
    ));

    fn spec() -> ObjectNamespaceProviderProfileSpec {
        ObjectNamespaceProviderProfileSpec {
            endpoint: "https://s3.example.com/".into(),
            region: "us-east-1".into(),
            bucket: "a3s-durable-cells".into(),
            prefix: "a3s/durable-cells".into(),
            virtual_hosted_style: false,
        }
    }

    #[test]
    fn canonical_acl_owns_only_non_secret_provider_semantics() {
        let profile = ObjectNamespaceProviderProfile::from_spec(spec()).expect("profile");
        assert_eq!(
            ObjectNamespaceProviderProfile::parse_acl(profile.canonical_acl()).expect("reparse"),
            profile
        );
        let namespace_id = StorageNamespaceId::new();
        assert_eq!(
            profile.namespace_prefix(namespace_id).expect("prefix"),
            format!("a3s/durable-cells/{namespace_id}")
        );
        for forbidden in ["access_key", "secret", "token"] {
            assert!(!profile.canonical_acl().contains(forbidden));
        }
    }

    #[test]
    fn shared_s0_profile_fixture_is_canonical_and_digest_locked() {
        let profile = ObjectNamespaceProviderProfile::parse_acl(PROFILE_FIXTURE).expect("profile");
        assert_eq!(
            format!("{}\n", profile.canonical_acl()),
            PROFILE_FIXTURE.replace("\r\n", "\n")
        );
        assert_eq!(
            profile.digest().as_str(),
            "sha256:3b223c671cb5a57cbc8c0b68d58746ac2417cc1c275fdf8aa7c39149092a4693"
        );
    }

    #[test]
    fn profile_rejects_insecure_or_ambiguous_storage_origins() {
        let mut invalid = spec();
        invalid.endpoint = "http://s3.example.com/".into();
        assert!(ObjectNamespaceProviderProfile::from_spec(invalid).is_err());

        let mut invalid = spec();
        invalid.endpoint = "https://user@s3.example.com/path".into();
        assert!(ObjectNamespaceProviderProfile::from_spec(invalid).is_err());

        let mut invalid = spec();
        invalid.prefix = "a3s/../cells".into();
        assert!(ObjectNamespaceProviderProfile::from_spec(invalid).is_err());

        let profile =
            ObjectNamespaceProviderProfile::from_spec(ObjectNamespaceProviderProfileSpec {
                endpoint: "https://S3.EXAMPLE.COM:443".into(),
                ..spec()
            })
            .expect("normalized profile");
        assert_eq!(profile.spec().endpoint, "https://s3.example.com");
    }

    #[test]
    fn parser_rejects_unknown_fields_and_noncanonical_storage() {
        let profile = ObjectNamespaceProviderProfile::from_spec(spec()).expect("profile");
        let unknown =
            profile
                .canonical_acl()
                .replacen("\n}", "\n  credential = \"forbidden\"\n}", 1);
        assert!(ObjectNamespaceProviderProfile::parse_acl(&unknown).is_err());
        assert!(ObjectNamespaceProviderProfile::restore(
            &format!("\n{}", profile.canonical_acl()),
            profile.digest().as_str()
        )
        .is_err());
    }
}

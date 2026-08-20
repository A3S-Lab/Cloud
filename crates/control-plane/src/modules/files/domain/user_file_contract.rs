use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, ProjectId, Sha256Digest, UserFileId, UserFileUploadId,
};
use a3s_acl::builder::{number, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const USER_FILE_ADMISSION_CONTRACT_SCHEMA: &str = "cloud.user-file.v1";
pub const USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES: usize = 64 * 1024;
pub const USER_FILE_MAX_BYTES: u64 = 512 * 1024 * 1024;
const USER_FILE_BLOCK: &str = "user_file";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserFileScanPolicy {
    Required,
}

impl UserFileScanPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "required" => Ok(Self::Required),
            _ => Err("unsupported UserFile scan policy".into()),
        }
    }
}

/// Logical reference to immutable user bytes.
///
/// Provider, bucket, credential, and local-path details are deliberately not
/// part of this contract. Files resolves the logical key through the one
/// shared immutable-object client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserFileContentReference {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub user_file_id: UserFileId,
    pub upload_id: UserFileUploadId,
    pub object_ref: String,
    pub digest: Sha256Digest,
    pub size_bytes: u64,
    pub media_type: String,
}

impl UserFileContentReference {
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        user_file_id: UserFileId,
        upload_id: UserFileUploadId,
        digest: Sha256Digest,
        size_bytes: u64,
        media_type: impl Into<String>,
    ) -> Result<Self, String> {
        let media_type = media_type.into();
        let object_ref = derive_object_ref(
            organization_id,
            project_id,
            user_file_id,
            upload_id,
            &digest,
        )?;
        let value = Self {
            organization_id,
            project_id,
            user_file_id,
            upload_id,
            object_ref,
            digest,
            size_bytes,
            media_type,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_non_nil(self.organization_id.as_uuid(), "organization")?;
        validate_non_nil(self.project_id.as_uuid(), "project")?;
        validate_non_nil(self.user_file_id.as_uuid(), "UserFile")?;
        validate_non_nil(self.upload_id.as_uuid(), "UserFile upload")?;
        if self.size_bytes == 0 || self.size_bytes > USER_FILE_MAX_BYTES {
            return Err(format!(
                "UserFile content size must be between 1 and {USER_FILE_MAX_BYTES} bytes"
            ));
        }
        if Sha256Digest::parse(self.digest.as_str())? != self.digest {
            return Err("UserFile content digest is not canonical".into());
        }
        validate_media_type(&self.media_type)?;
        if self.object_ref
            != derive_object_ref(
                self.organization_id,
                self.project_id,
                self.user_file_id,
                self.upload_id,
                &self.digest,
            )?
        {
            return Err("UserFile object reference changed its immutable identity".into());
        }
        Ok(())
    }

    pub(crate) fn storage_key(&self) -> Result<String, String> {
        self.validate()?;
        Ok(self.object_ref.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserFileAdmissionContractSpec {
    pub original_name: String,
    pub upload_expires_at: DateTime<Utc>,
    pub retention_until: DateTime<Utc>,
    pub scan_policy: UserFileScanPolicy,
    pub content: UserFileContentReference,
}

/// Canonical Files-owned upload and admission intent.
///
/// The contract carries metadata and a logical immutable-object reference, but
/// never bytes, provider configuration, a scan implementation, or an
/// Applications/Knowledge-local object key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserFileAdmissionContract {
    spec: UserFileAdmissionContractSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl UserFileAdmissionContract {
    pub fn from_spec(mut spec: UserFileAdmissionContractSpec) -> Result<Self, String> {
        validate_original_name(&spec.original_name)?;
        spec.content.validate()?;
        spec.upload_expires_at = canonical_timestamp(spec.upload_expires_at);
        spec.retention_until = canonical_timestamp(spec.retention_until);
        if spec.retention_until <= spec.upload_expires_at {
            return Err("UserFile retention must outlive its upload session".into());
        }
        let document = contract_document(&spec);
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES {
            return Err("UserFile admission ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated UserFile admission ACL is invalid: {error}"))?;
        let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
            format!("UserFile admission contract is not canonicalizable: {error}")
        })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > USER_FILE_ADMISSION_CONTRACT_MAX_ACL_BYTES {
            return Err("UserFile admission ACL size is invalid".into());
        }
        if source.replace("\r\n", "").contains('\r') {
            return Err("UserFile admission ACL contains a bare carriage return".into());
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized)
            .map_err(|error| format!("UserFile admission ACL is invalid: {error}"))?;
        let value = Self::from_spec(parse_contract(&document)?)?;
        if value.canonical_acl != normalized {
            return Err("UserFile admission ACL is not canonical".into());
        }
        Ok(value)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored UserFile admission ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        let restored = Self::restore(self.canonical_acl(), self.digest.as_str())?;
        if restored != *self {
            return Err("UserFile admission contract drifted from canonical ACL".into());
        }
        Ok(())
    }

    pub const fn spec(&self) -> &UserFileAdmissionContractSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn contract_document(spec: &UserFileAdmissionContractSpec) -> Document {
    let content = &spec.content;
    let content_block = BlockBuilder::new("content")
        .attr("content_digest", string(content.digest.as_str()))
        .attr("media_type", string(&content.media_type))
        .attr("object_ref", string(&content.object_ref))
        .attr(
            "organization_id",
            string(&content.organization_id.to_string()),
        )
        .attr("project_id", string(&content.project_id.to_string()))
        .attr("size_bytes", number(content.size_bytes as f64))
        .attr("upload_id", string(&content.upload_id.to_string()))
        .attr("user_file_id", string(&content.user_file_id.to_string()))
        .build();
    Document {
        blocks: vec![BlockBuilder::new(USER_FILE_BLOCK)
            .attr("original_name", string(&spec.original_name))
            .attr(
                "retention_until",
                string(
                    &spec
                        .retention_until
                        .to_rfc3339_opts(SecondsFormat::Micros, true),
                ),
            )
            .attr("scan_policy", string(spec.scan_policy.as_str()))
            .attr("schema", string(USER_FILE_ADMISSION_CONTRACT_SCHEMA))
            .attr(
                "upload_expires_at",
                string(
                    &spec
                        .upload_expires_at
                        .to_rfc3339_opts(SecondsFormat::Micros, true),
                ),
            )
            .nested_block(content_block)
            .build()],
    }
}

fn parse_contract(document: &Document) -> Result<UserFileAdmissionContractSpec, String> {
    if document.blocks.len() != 1 {
        return Err("UserFile admission must contain exactly one top-level block".into());
    }
    let root = &document.blocks[0];
    exact_shape(
        root,
        USER_FILE_BLOCK,
        &[
            "original_name",
            "retention_until",
            "scan_policy",
            "schema",
            "upload_expires_at",
        ],
        &["content"],
    )?;
    if required_string(root, "schema")? != USER_FILE_ADMISSION_CONTRACT_SCHEMA {
        return Err("UserFile admission schema is unsupported".into());
    }
    let content = exact_child(root, "content")?;
    exact_shape(
        content,
        "content",
        &[
            "content_digest",
            "media_type",
            "object_ref",
            "organization_id",
            "project_id",
            "size_bytes",
            "upload_id",
            "user_file_id",
        ],
        &[],
    )?;
    let upload_expires_at = required_timestamp(root, "upload_expires_at")?;
    let retention_until = required_timestamp(root, "retention_until")?;
    let reference = UserFileContentReference {
        organization_id: OrganizationId::from_uuid(required_uuid(content, "organization_id")?),
        project_id: ProjectId::from_uuid(required_uuid(content, "project_id")?),
        user_file_id: UserFileId::from_uuid(required_uuid(content, "user_file_id")?),
        upload_id: UserFileUploadId::from_uuid(required_uuid(content, "upload_id")?),
        object_ref: required_string(content, "object_ref")?,
        digest: required_digest(content, "content_digest")?,
        size_bytes: required_u64(content, "size_bytes")?,
        media_type: required_string(content, "media_type")?,
    };
    Ok(UserFileAdmissionContractSpec {
        original_name: required_string(root, "original_name")?,
        upload_expires_at,
        retention_until,
        scan_policy: UserFileScanPolicy::parse(&required_string(root, "scan_policy")?)?,
        content: reference,
    })
}

fn derive_object_ref(
    organization_id: OrganizationId,
    project_id: ProjectId,
    user_file_id: UserFileId,
    upload_id: UserFileUploadId,
    digest: &Sha256Digest,
) -> Result<String, String> {
    for (value, label) in [
        (organization_id.as_uuid(), "organization"),
        (project_id.as_uuid(), "project"),
        (user_file_id.as_uuid(), "UserFile"),
        (upload_id.as_uuid(), "UserFile upload"),
    ] {
        validate_non_nil(value, label)?;
    }
    let hexadecimal = digest
        .as_str()
        .strip_prefix("sha256:")
        .ok_or_else(|| "UserFile content digest is invalid".to_owned())?;
    Sha256Digest::parse(format!("sha256:{hexadecimal}"))?;
    Ok(format!(
        "organizations/{organization_id}/projects/{project_id}/files/{user_file_id}/uploads/{upload_id}/sha256/{hexadecimal}/content"
    ))
}

fn validate_original_name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value != value.trim()
        || value.chars().count() > 255
        || value.chars().any(char::is_control)
        || value.contains(['/', '\\'])
    {
        return Err("UserFile original name must contain 1 to 255 safe basename characters".into());
    }
    Ok(())
}

fn validate_media_type(value: &str) -> Result<(), String> {
    if value.len() > 127 || !value.is_ascii() || value.contains(['\0', '\r', '\n']) {
        return Err("UserFile media type is invalid".into());
    }
    let Some((kind, subtype)) = value.split_once('/') else {
        return Err("UserFile media type is invalid".into());
    };
    if kind.is_empty()
        || subtype.is_empty()
        || subtype.contains('/')
        || !kind.bytes().all(media_type_token)
        || !subtype.bytes().all(media_type_token)
    {
        return Err("UserFile media type is invalid".into());
    }
    Ok(())
}

fn media_type_token(value: u8) -> bool {
    value.is_ascii_alphanumeric()
        || matches!(
            value,
            b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
        )
}

fn validate_non_nil(value: Uuid, label: &str) -> Result<(), String> {
    if value.is_nil() {
        return Err(format!("{label} identity cannot be nil"));
    }
    Ok(())
}

fn exact_shape(
    block: &Block,
    name: &str,
    attributes: &[&str],
    children: &[&str],
) -> Result<(), String> {
    if block.name != name
        || !block.labels.is_empty()
        || block.attributes.len() != attributes.len()
        || block
            .attributes
            .keys()
            .any(|key| !attributes.contains(&key.as_str()))
        || block.blocks.len() != children.len()
        || block
            .blocks
            .iter()
            .any(|child| !children.contains(&child.name.as_str()))
    {
        return Err(format!("UserFile admission {name} block shape is invalid"));
    }
    Ok(())
}

fn exact_child<'a>(root: &'a Block, name: &str) -> Result<&'a Block, String> {
    let mut matches = root.blocks.iter().filter(|block| block.name == name);
    let value = matches
        .next()
        .ok_or_else(|| format!("UserFile admission {name} block is required"))?;
    if matches.next().is_some() {
        return Err(format!("UserFile admission {name} block must be unique"));
    }
    Ok(value)
}

fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("UserFile admission field {name:?} is required"))
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    match required_value(block, name)? {
        Value::String(value) => Ok(value.clone()),
        _ => Err(format!(
            "UserFile admission field {name:?} must be a string"
        )),
    }
}

fn required_uuid(block: &Block, name: &str) -> Result<Uuid, String> {
    let value = Uuid::parse_str(&required_string(block, name)?)
        .map_err(|_| format!("UserFile admission field {name:?} must be a UUID"))?;
    validate_non_nil(value, name)?;
    Ok(value)
}

fn required_digest(block: &Block, name: &str) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(required_string(block, name)?)
        .map_err(|_| format!("UserFile admission field {name:?} must be a SHA-256 digest"))
}

fn required_timestamp(block: &Block, name: &str) -> Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(&required_string(block, name)?)
        .map_err(|_| format!("UserFile admission field {name:?} must be an RFC 3339 timestamp"))
        .map(|value| value.with_timezone(&Utc))
}

fn required_u64(block: &Block, name: &str) -> Result<u64, String> {
    let Value::Number(value) = required_value(block, name)? else {
        return Err(format!(
            "UserFile admission field {name:?} must be an integer"
        ));
    };
    if !value.is_finite()
        || value.fract() != 0.0
        || *value <= 0.0
        || *value > USER_FILE_MAX_BYTES as f64
    {
        return Err(format!(
            "UserFile admission field {name:?} must be a bounded positive integer"
        ));
    }
    Ok(*value as u64)
}

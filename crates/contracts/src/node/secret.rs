use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use uuid::Uuid;
use zeroize::Zeroize;

const REFERENCE_PREFIX: &str = "a3s-cloud-secret://";
const MAXIMUM_MATERIAL_BYTES: usize = 1024 * 1024;
const MAXIMUM_MATERIAL_TTL: Duration = Duration::seconds(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudSecretReference {
    pub workload_revision_id: Uuid,
    pub secret_id: Uuid,
    pub version: u64,
}

impl CloudSecretReference {
    pub fn new(workload_revision_id: Uuid, secret_id: Uuid, version: u64) -> Result<Self, String> {
        let reference = Self {
            workload_revision_id,
            secret_id,
            version,
        };
        reference.validate()?;
        Ok(reference)
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        if value.len() > 256 || value.contains(['\0', '\r', '\n', '?', '#']) {
            return Err("Cloud Secret reference is invalid".into());
        }
        let value = value
            .strip_prefix(REFERENCE_PREFIX)
            .ok_or_else(|| "Cloud Secret reference uses an unsupported scheme".to_owned())?;
        let mut segments = value.split('/');
        let workload_revision_id = segments
            .next()
            .ok_or_else(|| "Cloud Secret reference omits its workload revision".to_owned())
            .and_then(parse_uuid)?;
        let secret_id = segments
            .next()
            .ok_or_else(|| "Cloud Secret reference omits its Secret identity".to_owned())
            .and_then(parse_uuid)?;
        let version = segments
            .next()
            .ok_or_else(|| "Cloud Secret reference omits its version".to_owned())?
            .parse::<u64>()
            .map_err(|_| "Cloud Secret reference version is invalid".to_owned())?;
        if segments.next().is_some() {
            return Err("Cloud Secret reference has unexpected path segments".into());
        }
        Self::new(workload_revision_id, secret_id, version)
    }

    pub fn validate(&self) -> Result<(), String> {
        super::validate_uuid("workload_revision_id", self.workload_revision_id)?;
        super::validate_uuid("secret_id", self.secret_id)?;
        if self.version == 0 {
            return Err("Secret version must be positive".into());
        }
        Ok(())
    }
}

impl Display for CloudSecretReference {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{REFERENCE_PREFIX}{}/{}/{}",
            self.workload_revision_id, self.secret_id, self.version
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSecretMaterialRequest {
    pub schema: String,
    pub node_id: Uuid,
    pub reference: CloudSecretReference,
}

impl NodeSecretMaterialRequest {
    pub const SCHEMA: &'static str = "a3s.cloud.node-secret-material-request.v1";

    pub fn new(node_id: Uuid, reference: CloudSecretReference) -> Result<Self, String> {
        let request = Self {
            schema: Self::SCHEMA.into(),
            node_id,
            reference,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node Secret material request schema {:?}",
                self.schema
            ));
        }
        super::validate_uuid("node_id", self.node_id)?;
        self.reference.validate()
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSecretMaterialResponse {
    pub schema: String,
    pub node_id: Uuid,
    pub reference: CloudSecretReference,
    value_base64: String,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
}

impl NodeSecretMaterialResponse {
    pub const SCHEMA: &'static str = "a3s.cloud.node-secret-material-response.v1";

    pub fn new(
        node_id: Uuid,
        reference: CloudSecretReference,
        value: &[u8],
        issued_at: DateTime<Utc>,
        not_after: DateTime<Utc>,
    ) -> Result<Self, String> {
        if value.is_empty() || value.len() > MAXIMUM_MATERIAL_BYTES {
            return Err("node Secret material must contain between 1 byte and 1 MiB".into());
        }
        let response = Self {
            schema: Self::SCHEMA.into(),
            node_id,
            reference,
            value_base64: STANDARD.encode(value),
            issued_at,
            not_after,
        };
        response.validate()?;
        Ok(response)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_metadata()?;
        let mut decoded = STANDARD
            .decode(self.value_base64.as_bytes())
            .map_err(|_| "node Secret material encoding is invalid".to_owned())?;
        let valid_size = !decoded.is_empty() && decoded.len() <= MAXIMUM_MATERIAL_BYTES;
        decoded.zeroize();
        if !valid_size {
            return Err("node Secret material size is invalid".into());
        }
        Ok(())
    }

    pub fn decode_at(&self, now: DateTime<Utc>) -> Result<Vec<u8>, String> {
        self.validate_metadata()?;
        if now < self.issued_at || now >= self.not_after {
            return Err("node Secret material is not currently valid".into());
        }
        let mut decoded = STANDARD
            .decode(self.value_base64.as_bytes())
            .map_err(|_| "node Secret material encoding is invalid".to_owned())?;
        if decoded.is_empty() || decoded.len() > MAXIMUM_MATERIAL_BYTES {
            decoded.zeroize();
            return Err("node Secret material size is invalid".into());
        }
        Ok(decoded)
    }

    fn validate_metadata(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node Secret material response schema {:?}",
                self.schema
            ));
        }
        super::validate_uuid("node_id", self.node_id)?;
        self.reference.validate()?;
        if self.not_after <= self.issued_at
            || self.not_after - self.issued_at > MAXIMUM_MATERIAL_TTL
        {
            return Err("node Secret material validity period is invalid".into());
        }
        Ok(())
    }
}

impl std::fmt::Debug for NodeSecretMaterialResponse {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NodeSecretMaterialResponse")
            .field("schema", &self.schema)
            .field("node_id", &self.node_id)
            .field("reference", &self.reference)
            .field("value_base64", &"<redacted-secret-material>")
            .field("issued_at", &self.issued_at)
            .field("not_after", &self.not_after)
            .finish()
    }
}

impl Drop for NodeSecretMaterialResponse {
    fn drop(&mut self) {
        self.value_base64.zeroize();
    }
}

fn parse_uuid(value: &str) -> Result<Uuid, String> {
    Uuid::parse_str(value).map_err(|_| "Cloud Secret reference UUID is invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_strict_typed_and_canonical() {
        let reference =
            CloudSecretReference::new(Uuid::now_v7(), Uuid::now_v7(), 7).expect("reference");
        assert_eq!(
            CloudSecretReference::parse(&reference.to_string()).expect("parsed reference"),
            reference
        );
        assert!(CloudSecretReference::parse("secret://plaintext").is_err());
        assert!(CloudSecretReference::parse(&format!("{reference}/extra")).is_err());
    }

    #[test]
    fn material_debug_is_redacted_and_validity_is_bounded() {
        let now = Utc::now();
        let reference =
            CloudSecretReference::new(Uuid::now_v7(), Uuid::now_v7(), 1).expect("reference");
        let response = NodeSecretMaterialResponse::new(
            Uuid::now_v7(),
            reference,
            b"never-print-this",
            now,
            now + Duration::seconds(30),
        )
        .expect("material response");
        let debug = format!("{response:?}");
        assert!(!debug.contains("never-print-this"));
        assert!(!debug.contains(&STANDARD.encode(b"never-print-this")));
        assert_eq!(
            response.decode_at(now).expect("decoded material"),
            b"never-print-this"
        );
    }
}

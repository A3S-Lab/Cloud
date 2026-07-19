use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, GatewayCertificateId, NodeCommandId, NodeId, OrganizationId,
};
use a3s_cloud_contracts::{GatewayAckState, GatewayCertificateRequest, NodeGatewayAck};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayCertificateState {
    Provisioning,
    Issued,
    Ready,
    Failed,
    Revoked,
}

impl GatewayCertificateState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Provisioning => "provisioning",
            Self::Issued => "issued",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Revoked => "revoked",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "provisioning" => Ok(Self::Provisioning),
            "issued" => Ok(Self::Issued),
            "ready" => Ok(Self::Ready),
            "failed" => Ok(Self::Failed),
            "revoked" => Ok(Self::Revoked),
            _ => Err(format!("unsupported Gateway certificate state {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayCertificateMaterial {
    pub serial_number: String,
    pub fingerprint: String,
    pub certificate_pem: String,
    pub ca_bundle_pem: String,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl GatewayCertificateMaterial {
    pub fn validate(&self) -> Result<(), String> {
        validate_single_line(
            "Gateway certificate serial number",
            &self.serial_number,
            512,
        )?;
        validate_sha256(&self.fingerprint)?;
        validate_pem("Gateway certificate", &self.certificate_pem, "CERTIFICATE")?;
        validate_pem("Gateway CA bundle", &self.ca_bundle_pem, "CERTIFICATE")?;
        if canonical_timestamp(self.expires_at) <= canonical_timestamp(self.issued_at) {
            return Err("Gateway certificate expiry must follow its issue time".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayCertificate {
    pub id: GatewayCertificateId,
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub domain_claim_ids: Vec<DomainClaimId>,
    pub gateway_revision: u64,
    pub gateway_command_id: NodeCommandId,
    pub snapshot_digest: String,
    pub request: GatewayCertificateRequest,
    pub state: GatewayCertificateState,
    pub csr_digest: Option<String>,
    pub material: Option<GatewayCertificateMaterial>,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub ready_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl GatewayCertificate {
    #[allow(clippy::too_many_arguments)]
    pub fn provision(
        id: GatewayCertificateId,
        organization_id: OrganizationId,
        node_id: NodeId,
        domain_claim_ids: Vec<DomainClaimId>,
        gateway_revision: u64,
        gateway_command_id: NodeCommandId,
        snapshot_digest: String,
        request: GatewayCertificateRequest,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        request.validate()?;
        if request.certificate_id != id.as_uuid()
            || domain_claim_ids.is_empty()
            || domain_claim_ids.windows(2).any(|ids| ids[0] >= ids[1])
            || gateway_revision == 0
        {
            return Err("Gateway certificate provisioning identity is invalid".into());
        }
        validate_sha256(&snapshot_digest)?;
        let created_at = canonical_timestamp(created_at);
        Ok(Self {
            id,
            organization_id,
            node_id,
            domain_claim_ids,
            gateway_revision,
            gateway_command_id,
            snapshot_digest,
            request,
            state: GatewayCertificateState::Provisioning,
            csr_digest: None,
            material: None,
            failure: None,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
            ready_at: None,
            revoked_at: None,
        })
    }

    pub fn record_issued(
        &mut self,
        csr_digest: String,
        mut material: GatewayCertificateMaterial,
        recorded_at: DateTime<Utc>,
    ) -> Result<(), String> {
        validate_sha256(&csr_digest)?;
        material.validate()?;
        material.issued_at = canonical_timestamp(material.issued_at);
        material.expires_at = canonical_timestamp(material.expires_at);
        let recorded_at = canonical_timestamp(recorded_at);
        self.ensure_time(recorded_at)?;
        if material.issued_at > recorded_at || material.expires_at <= recorded_at {
            return Err("Gateway certificate is not valid at its issue projection time".into());
        }
        if self.state == GatewayCertificateState::Issued
            && self.csr_digest.as_deref() == Some(csr_digest.as_str())
            && self.material.as_ref() == Some(&material)
        {
            return Ok(());
        }
        if self.state != GatewayCertificateState::Provisioning {
            return Err("Gateway certificate cannot record issuance from its current state".into());
        }
        self.state = GatewayCertificateState::Issued;
        self.csr_digest = Some(csr_digest);
        self.material = Some(material);
        self.aggregate_version += 1;
        self.updated_at = recorded_at;
        Ok(())
    }

    pub fn fail_provisioning(
        &mut self,
        csr_digest: String,
        failure: impl Into<String>,
        failed_at: DateTime<Utc>,
    ) -> Result<(), String> {
        validate_sha256(&csr_digest)?;
        let failure = sanitize_reason(
            failure,
            "Gateway certificate provisioning failure is invalid",
        )?;
        let failed_at = canonical_timestamp(failed_at);
        self.ensure_time(failed_at)?;
        if self.state == GatewayCertificateState::Failed
            && self.csr_digest.as_deref() == Some(csr_digest.as_str())
            && self.failure.as_deref() == Some(failure.as_str())
        {
            return Ok(());
        }
        if self.state != GatewayCertificateState::Provisioning {
            return Err(
                "Gateway certificate cannot fail provisioning from its current state".into(),
            );
        }
        self.state = GatewayCertificateState::Failed;
        self.csr_digest = Some(csr_digest);
        self.failure = Some(failure);
        self.aggregate_version += 1;
        self.updated_at = failed_at;
        Ok(())
    }

    pub fn apply_gateway_acknowledgement(
        &mut self,
        acknowledgement: &NodeGatewayAck,
    ) -> Result<(), String> {
        acknowledgement.validate()?;
        if acknowledgement.node_id != self.node_id.as_uuid()
            || acknowledgement.command_id != self.gateway_command_id.as_uuid()
            || acknowledgement.revision != self.gateway_revision
            || acknowledgement.snapshot_digest != self.snapshot_digest
        {
            return Err(
                "Gateway acknowledgement does not match the certificate publication".into(),
            );
        }
        let acknowledged_at = canonical_timestamp(acknowledgement.acknowledged_at);
        self.ensure_time(acknowledged_at)?;
        match acknowledgement.state {
            GatewayAckState::Applied => {
                if self.state == GatewayCertificateState::Ready {
                    return Ok(());
                }
                if self.state != GatewayCertificateState::Issued
                    || self.material.as_ref().is_none_or(|material| {
                        material.issued_at > acknowledged_at
                            || material.expires_at <= acknowledged_at
                    })
                {
                    return Err(
                        "Gateway certificate must be issued and valid before becoming ready".into(),
                    );
                }
                self.state = GatewayCertificateState::Ready;
                self.ready_at = Some(acknowledged_at);
                self.failure = None;
            }
            GatewayAckState::Rejected => {
                let failure = acknowledgement
                    .message
                    .clone()
                    .ok_or_else(|| "rejected Gateway certificate requires a failure".to_string())?;
                if self.state == GatewayCertificateState::Failed
                    && self.failure.as_deref() == Some(failure.as_str())
                {
                    return Ok(());
                }
                if !matches!(
                    self.state,
                    GatewayCertificateState::Provisioning | GatewayCertificateState::Issued
                ) {
                    return Err("Gateway certificate cannot fail from its current state".into());
                }
                self.state = GatewayCertificateState::Failed;
                self.failure = Some(failure);
            }
        }
        self.aggregate_version += 1;
        self.updated_at = acknowledged_at;
        Ok(())
    }

    pub fn revoke(
        &mut self,
        reason: impl Into<String>,
        revoked_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let reason = sanitize_reason(reason, "Gateway certificate revocation reason is invalid")?;
        let revoked_at = canonical_timestamp(revoked_at);
        self.ensure_time(revoked_at)?;
        if self.state == GatewayCertificateState::Revoked
            && self.failure.as_deref() == Some(reason.as_str())
        {
            return Ok(());
        }
        if self.state != GatewayCertificateState::Ready {
            return Err("only a ready Gateway certificate can be revoked".into());
        }
        self.state = GatewayCertificateState::Revoked;
        self.failure = Some(reason);
        self.aggregate_version += 1;
        self.updated_at = revoked_at;
        self.revoked_at = Some(revoked_at);
        Ok(())
    }

    fn ensure_time(&self, at: DateTime<Utc>) -> Result<(), String> {
        if at < self.updated_at {
            return Err("Gateway certificate transition time regressed".into());
        }
        Ok(())
    }
}

fn sanitize_reason(reason: impl Into<String>, error: &'static str) -> Result<String, String> {
    let reason = reason.into().replace(['\0', '\r', '\n'], " ");
    let reason = reason.trim();
    if reason.is_empty() || reason.len() > 4096 {
        return Err(error.into());
    }
    Ok(reason.into())
}

fn validate_single_line(label: &str, value: &str, maximum: usize) -> Result<(), String> {
    if value.trim().is_empty()
        || value.trim() != value
        || value.len() > maximum
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{label} must be a bounded single-line value"));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err("Gateway certificate digest must use sha256".into());
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Gateway certificate digest must contain 64 lowercase hex characters".into());
    }
    Ok(())
}

fn validate_pem(label: &str, value: &str, kind: &str) -> Result<(), String> {
    if value.len() > 256 * 1024
        || !value.starts_with(&format!("-----BEGIN {kind}-----\n"))
        || !value.ends_with(&format!("-----END {kind}-----\n"))
        || value.contains('\0')
    {
        return Err(format!("{label} is not a bounded PEM value"));
    }
    Ok(())
}

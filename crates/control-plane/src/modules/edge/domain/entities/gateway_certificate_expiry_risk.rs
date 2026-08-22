use crate::modules::edge::domain::{
    GatewayCertificate, GatewayCertificateState, Route, RouteState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayCertificateId, NodeId, OrganizationId, RouteId,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub const GATEWAY_CERTIFICATE_EXPIRY_RISK_WINDOW_SECONDS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayCertificateExpiryRiskState {
    AtRisk,
    Clear,
}

impl GatewayCertificateExpiryRiskState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AtRisk => "at_risk",
            Self::Clear => "clear",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "at_risk" => Ok(Self::AtRisk),
            "clear" => Ok(Self::Clear),
            _ => Err(format!(
                "unsupported Gateway certificate expiry-risk state {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayCertificateExpiryRisk {
    pub organization_id: OrganizationId,
    pub route_id: RouteId,
    pub node_id: NodeId,
    pub state: GatewayCertificateExpiryRiskState,
    pub active_certificate_id: GatewayCertificateId,
    pub active_certificate_expires_at: DateTime<Utc>,
    pub gateway_revision: u64,
    pub generation: u64,
    pub previous_at_risk_certificate_id: Option<GatewayCertificateId>,
    pub previous_at_risk_certificate_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl GatewayCertificateExpiryRisk {
    pub fn observe(
        previous: Option<&Self>,
        route: &Route,
        certificate: &GatewayCertificate,
        observed_at: DateTime<Utc>,
    ) -> Result<Option<Self>, String> {
        let observed_at = canonical_timestamp(observed_at);
        let (gateway_revision, expires_at) =
            validate_active_binding(route, certificate, observed_at)?;
        let risk_deadline = expiry_risk_deadline(observed_at)?;

        if let Some(previous) = previous {
            previous.validate()?;
            if previous.organization_id != route.organization_id
                || previous.route_id != route.id
                || previous.node_id != route.gateway_node_id
                || previous.updated_at > observed_at
            {
                return Err(
                    "Gateway certificate expiry-risk observation changed scope or moved backward"
                        .into(),
                );
            }
        }

        if expires_at <= risk_deadline {
            if previous.is_some_and(|risk| {
                risk.state == GatewayCertificateExpiryRiskState::AtRisk
                    && risk.active_certificate_id == certificate.id
                    && risk.active_certificate_expires_at == expires_at
            }) {
                return Ok(None);
            }
            let generation = next_generation(previous)?;
            let risk = Self {
                organization_id: route.organization_id,
                route_id: route.id,
                node_id: route.gateway_node_id,
                state: GatewayCertificateExpiryRiskState::AtRisk,
                active_certificate_id: certificate.id,
                active_certificate_expires_at: expires_at,
                gateway_revision,
                generation,
                previous_at_risk_certificate_id: None,
                previous_at_risk_certificate_expires_at: None,
                created_at: previous.map_or(observed_at, |risk| risk.created_at),
                updated_at: observed_at,
            };
            risk.validate()?;
            return Ok(Some(risk));
        }

        let Some(previous) =
            previous.filter(|risk| risk.state == GatewayCertificateExpiryRiskState::AtRisk)
        else {
            return Ok(None);
        };
        if previous.active_certificate_id == certificate.id {
            return Err(
                "Gateway certificate expiry risk cannot clear the same active certificate".into(),
            );
        }
        let risk = Self {
            organization_id: route.organization_id,
            route_id: route.id,
            node_id: route.gateway_node_id,
            state: GatewayCertificateExpiryRiskState::Clear,
            active_certificate_id: certificate.id,
            active_certificate_expires_at: expires_at,
            gateway_revision,
            generation: next_generation(Some(previous))?,
            previous_at_risk_certificate_id: Some(previous.active_certificate_id),
            previous_at_risk_certificate_expires_at: Some(previous.active_certificate_expires_at),
            created_at: previous.created_at,
            updated_at: observed_at,
        };
        risk.validate()?;
        Ok(Some(risk))
    }

    pub fn validate(&self) -> Result<(), String> {
        let timestamps_are_canonical = canonical_timestamp(self.active_certificate_expires_at)
            == self.active_certificate_expires_at
            && canonical_timestamp(self.created_at) == self.created_at
            && canonical_timestamp(self.updated_at) == self.updated_at
            && self
                .previous_at_risk_certificate_expires_at
                .is_none_or(|expires_at| canonical_timestamp(expires_at) == expires_at);
        if self.organization_id.as_uuid().is_nil()
            || self.route_id.as_uuid().is_nil()
            || self.node_id.as_uuid().is_nil()
            || self.active_certificate_id.as_uuid().is_nil()
            || self.gateway_revision == 0
            || self.generation == 0
            || self.updated_at < self.created_at
            || !timestamps_are_canonical
        {
            return Err("Gateway certificate expiry-risk identity is invalid".into());
        }
        let risk_deadline = expiry_risk_deadline(self.updated_at)?;
        let state_is_consistent = match self.state {
            GatewayCertificateExpiryRiskState::AtRisk => {
                self.active_certificate_expires_at <= risk_deadline
                    && self.previous_at_risk_certificate_id.is_none()
                    && self.previous_at_risk_certificate_expires_at.is_none()
            }
            GatewayCertificateExpiryRiskState::Clear => {
                self.active_certificate_expires_at > risk_deadline
                    && self
                        .previous_at_risk_certificate_id
                        .is_some_and(|id| id != self.active_certificate_id)
                    && self.previous_at_risk_certificate_expires_at.is_some()
            }
        };
        if !state_is_consistent {
            return Err("Gateway certificate expiry-risk state is inconsistent".into());
        }
        Ok(())
    }
}

pub fn expiry_risk_deadline(observed_at: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
    canonical_timestamp(observed_at)
        .checked_add_signed(Duration::seconds(
            GATEWAY_CERTIFICATE_EXPIRY_RISK_WINDOW_SECONDS as i64,
        ))
        .ok_or_else(|| "Gateway certificate expiry-risk window exceeds supported time".into())
}

fn validate_active_binding(
    route: &Route,
    certificate: &GatewayCertificate,
    observed_at: DateTime<Utc>,
) -> Result<(u64, DateTime<Utc>), String> {
    let gateway_revision = route.gateway_revision.ok_or_else(|| {
        "active Gateway certificate expiry-risk Route omitted its revision".to_owned()
    })?;
    let material = certificate.material.as_ref().ok_or_else(|| {
        "active Gateway certificate expiry-risk observation omitted material".to_owned()
    })?;
    certificate.request.validate()?;
    material.validate()?;
    let expires_at = canonical_timestamp(material.expires_at);
    if route.state != RouteState::Active
        || route.organization_id != certificate.organization_id
        || route.gateway_node_id != certificate.node_id
        || route.gateway_certificate_id != Some(certificate.id)
        || route.gateway_command_id.is_none()
        || route.snapshot_digest.is_none()
        || route.failure.is_some()
        || route.activated_at.is_none()
        || route.domain_claim_id.is_none()
        || route.domain_pattern.is_none()
        || certificate.state != GatewayCertificateState::Ready
        || certificate.request.certificate_id != certificate.id.as_uuid()
        || certificate.failure.is_some()
        || certificate.ready_at.is_none()
        || certificate.revoked_at.is_some()
        || gateway_revision == 0
        || route.updated_at > observed_at
        || certificate.updated_at > observed_at
        || expires_at != material.expires_at
    {
        return Err("Gateway certificate expiry-risk active binding is inconsistent".into());
    }
    Ok((gateway_revision, expires_at))
}

fn next_generation(previous: Option<&GatewayCertificateExpiryRisk>) -> Result<u64, String> {
    previous.map_or(Ok(1), |risk| {
        risk.generation
            .checked_add(1)
            .ok_or_else(|| "Gateway certificate expiry-risk generation space is exhausted".into())
    })
}

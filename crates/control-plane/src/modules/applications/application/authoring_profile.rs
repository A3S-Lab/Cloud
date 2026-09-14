//! Immutable Applications-owned preset authoring profiles.
//!
//! `APP0.2-C29` freezes the profile that publishes over C4 wrapper evidence.
//! Application head writes, Identity credentials, and management delivery stay
//! later.

use super::preset_workflow_port::{ApplicationPresetTarget, ApplicationPresetWorkflowRequest};
use crate::modules::applications::domain::{
    digest_json, ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
    ApplicationReleaseContractSpec, ApplicationResponseMode, ApplicationWorkflowBinding,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ApplicationAuthoringProfileId, ApplicationId, OrganizationId, PrincipalId,
    ProjectId, Sha256Digest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const AUTHORING_PROFILE_IDENTITY: &str = "application-authoring-profile:v1";

/// One immutable preset authoring profile bound to an exact release slot.
///
/// Profiles never write Workflow storage. They only carry product policy and an
/// exact capability target so C4 can publish wrapper evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationAuthoringProfile {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub application_release_number: u64,
    pub experience: ApplicationExperience,
    pub audience: ApplicationAudience,
    pub delivery: ApplicationDeliveryPolicy,
    pub target: ApplicationPresetTarget,
    pub presentation_digest: Sha256Digest,
    pub id: ApplicationAuthoringProfileId,
    pub created_at: DateTime<Utc>,
}

impl ApplicationAuthoringProfile {
    pub fn create(
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        application_release_number: u64,
        experience: ApplicationExperience,
        audience: ApplicationAudience,
        delivery: ApplicationDeliveryPolicy,
        target: ApplicationPresetTarget,
        presentation_digest: Sha256Digest,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let delivery = normalize_delivery(experience, delivery)?;
        validate_experience_target(experience, &target)?;
        if Sha256Digest::parse(presentation_digest.as_str())? != presentation_digest {
            return Err("Application authoring presentation digest is not canonical".into());
        }
        let created_at = canonical_timestamp(created_at);
        let id = Self::deterministic_id(
            organization_id,
            project_id,
            application_id,
            application_release_number,
            experience,
            audience,
            &delivery,
            &target,
            &presentation_digest,
        )?;
        let value = Self {
            organization_id,
            project_id,
            application_id,
            application_release_number,
            experience,
            audience,
            delivery,
            target,
            presentation_digest,
            id,
            created_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.created_at = canonical_timestamp(self.created_at);
        self.delivery = normalize_delivery(self.experience, self.delivery.clone())?;
        self.validate()?;
        Ok(self)
    }

    pub fn deterministic_id(
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        application_release_number: u64,
        experience: ApplicationExperience,
        audience: ApplicationAudience,
        delivery: &ApplicationDeliveryPolicy,
        target: &ApplicationPresetTarget,
        presentation_digest: &Sha256Digest,
    ) -> Result<ApplicationAuthoringProfileId, String> {
        if organization_id.as_uuid().is_nil()
            || project_id.as_uuid().is_nil()
            || application_id.as_uuid().is_nil()
            || application_release_number == 0
        {
            return Err("Application authoring profile identity inputs are invalid".into());
        }
        let material = digest_json(&serde_json::json!({
            "audience": audience.as_str(),
            "applicationId": application_id.to_string(),
            "applicationReleaseNumber": application_release_number,
            "delivery": {
                "interactionMode": delivery.interaction_mode.as_str(),
                "responseModes": delivery
                    .response_modes
                    .iter()
                    .map(|mode| mode.as_str())
                    .collect::<Vec<_>>(),
            },
            "experience": experience.as_str(),
            "organizationId": organization_id.to_string(),
            "presentationDigest": presentation_digest.as_str(),
            "projectId": project_id.to_string(),
            "target": target,
            "v": AUTHORING_PROFILE_IDENTITY,
        }))?;
        Ok(ApplicationAuthoringProfileId::from_uuid(Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            material.as_str().as_bytes(),
        )))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.application_release_number == 0
            || self.id.as_uuid().is_nil()
        {
            return Err("Application authoring profile identity is invalid".into());
        }
        let delivery = normalize_delivery(self.experience, self.delivery.clone())?;
        if delivery != self.delivery {
            return Err("Application authoring delivery is not canonical".into());
        }
        validate_experience_target(self.experience, &self.target)?;
        if Sha256Digest::parse(self.presentation_digest.as_str())? != self.presentation_digest {
            return Err("Application authoring presentation digest is not canonical".into());
        }
        let expected = Self::deterministic_id(
            self.organization_id,
            self.project_id,
            self.application_id,
            self.application_release_number,
            self.experience,
            self.audience,
            &self.delivery,
            &self.target,
            &self.presentation_digest,
        )?;
        if expected != self.id {
            return Err("Application authoring profile identity drifted".into());
        }
        Ok(())
    }

    pub fn to_preset_request(
        &self,
        actor_principal_id: PrincipalId,
        idempotency_key: String,
        request_id: Uuid,
    ) -> Result<ApplicationPresetWorkflowRequest, String> {
        self.validate()?;
        if actor_principal_id.as_uuid().is_nil() || request_id.is_nil() {
            return Err("Application authoring publication actor identity is invalid".into());
        }
        let request = ApplicationPresetWorkflowRequest {
            organization_id: self.organization_id,
            project_id: self.project_id,
            application_id: self.application_id,
            application_release_number: self.application_release_number,
            experience: self.experience,
            target: self.target.clone(),
            actor_principal_id,
            idempotency_key,
            request_id,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn to_release_contract_spec(
        &self,
        workflow: ApplicationWorkflowBinding,
    ) -> Result<ApplicationReleaseContractSpec, String> {
        self.validate()?;
        workflow.validate()?;
        Ok(ApplicationReleaseContractSpec {
            experience: self.experience,
            audience: self.audience,
            delivery: self.delivery.clone(),
            workflow,
            presentation_digest: self.presentation_digest.clone(),
        })
    }
}

fn validate_experience_target(
    experience: ApplicationExperience,
    target: &ApplicationPresetTarget,
) -> Result<(), String> {
    match (experience, target) {
        (
            ApplicationExperience::Chatbot | ApplicationExperience::TextGenerator,
            ApplicationPresetTarget::ModelRevision(_),
        )
        | (
            ApplicationExperience::ClassicAgent | ApplicationExperience::NewAgent,
            ApplicationPresetTarget::AgentRelease(_),
        ) => Ok(()),
        (ApplicationExperience::Chatflow | ApplicationExperience::Workflow, _) => {
            Err("Chatflow and Workflow require an exact user-authored Workflow revision".into())
        }
        _ => Err("Application authoring profile target does not match its experience".into()),
    }
}

fn normalize_delivery(
    experience: ApplicationExperience,
    delivery: ApplicationDeliveryPolicy,
) -> Result<ApplicationDeliveryPolicy, String> {
    if delivery.interaction_mode != experience.interaction_mode() {
        return Err("Application delivery mode does not match its experience".into());
    }
    if delivery.response_modes.is_empty() || delivery.response_modes.len() > 3 {
        return Err("Application delivery must admit between one and three response modes".into());
    }
    let unique = delivery
        .response_modes
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if unique.len() != delivery.response_modes.len() {
        return Err("Application delivery contains duplicate response modes".into());
    }
    let _ = ApplicationResponseMode::Blocking;
    Ok(ApplicationDeliveryPolicy {
        interaction_mode: experience.interaction_mode(),
        response_modes: unique.into_iter().collect(),
    })
}

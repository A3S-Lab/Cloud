//! APP0.4-C1: Claim path for proven application-mode experiences.
//!
//! Freezes the non-invented contract: Chatbot / Text Generator / classic Agent /
//! New Agent share one authoring-profile ? release-contract path (APP0.2-C29).
//! Chatflow and Workflow fail closed on that preset path. Does not invent
//! mode-specific controllers or flip Chatflow/Workflow availability.

use super::authoring_profile::ApplicationAuthoringProfile;
use super::preset_workflow_port::{
    ApplicationPresetAgentRelease, ApplicationPresetModelRevision, ApplicationPresetTarget,
};
use crate::modules::applications::domain::{
    ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
    ApplicationInteractionMode, ApplicationResponseMode,
};
use crate::modules::shared_kernel::domain::{
    ApplicationId, AssetId, AssetReleaseId, OrganizationId, ProjectId, Sha256Digest,
};
use chrono::{TimeZone, Utc};
use uuid::Uuid;

fn digest(marker: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64))).expect("digest")
}

fn model_target(marker: char) -> ApplicationPresetTarget {
    ApplicationPresetTarget::ModelRevision(ApplicationPresetModelRevision {
        model_id: Uuid::from_u128(u128::from(marker as u8)),
        revision: format!("model-{marker}"),
        digest: digest(marker),
        capability: "model.invoke".into(),
    })
}

fn agent_target(marker: char) -> ApplicationPresetTarget {
    ApplicationPresetTarget::AgentRelease(ApplicationPresetAgentRelease {
        asset_id: AssetId::from_uuid(Uuid::from_u128(0x22)),
        asset_release_id: AssetReleaseId::from_uuid(Uuid::from_u128(0x33)),
        digest: digest(marker),
        capability: "agent.execute".into(),
    })
}

fn delivery(experience: ApplicationExperience) -> ApplicationDeliveryPolicy {
    ApplicationDeliveryPolicy {
        interaction_mode: experience.interaction_mode(),
        response_modes: vec![ApplicationResponseMode::Blocking],
    }
}

fn try_profile(
    experience: ApplicationExperience,
    target: ApplicationPresetTarget,
) -> Result<(), String> {
    ApplicationAuthoringProfile::create(
        OrganizationId::from_uuid(Uuid::from_u128(1)),
        ProjectId::from_uuid(Uuid::from_u128(2)),
        ApplicationId::from_uuid(Uuid::from_u128(3)),
        1,
        experience,
        ApplicationAudience::ProjectMembers,
        delivery(experience),
        target,
        digest('a'),
        Utc.with_ymd_and_hms(2026, 9, 15, 12, 0, 0)
            .single()
            .expect("ts"),
    )
    .map(|_| ())
}

#[test]
fn six_experiences_freeze_interaction_modes_and_wire_names() {
    let cases = [
        (
            ApplicationExperience::Chatbot,
            "chatbot",
            ApplicationInteractionMode::Conversation,
        ),
        (
            ApplicationExperience::TextGenerator,
            "text_generator",
            ApplicationInteractionMode::Invocation,
        ),
        (
            ApplicationExperience::ClassicAgent,
            "classic_agent",
            ApplicationInteractionMode::Conversation,
        ),
        (
            ApplicationExperience::NewAgent,
            "new_agent",
            ApplicationInteractionMode::Conversation,
        ),
        (
            ApplicationExperience::Chatflow,
            "chatflow",
            ApplicationInteractionMode::Conversation,
        ),
        (
            ApplicationExperience::Workflow,
            "workflow",
            ApplicationInteractionMode::Invocation,
        ),
    ];
    for (experience, name, mode) in cases {
        assert_eq!(experience.as_str(), name);
        assert_eq!(ApplicationExperience::parse(name).expect("parse"), experience);
        assert_eq!(experience.interaction_mode(), mode);
    }
}

#[test]
fn proven_preset_modes_accept_authoring_profile_claim_path() {
    let proven = [
        (ApplicationExperience::Chatbot, model_target('1')),
        (ApplicationExperience::TextGenerator, model_target('2')),
        (ApplicationExperience::ClassicAgent, agent_target('3')),
        (ApplicationExperience::NewAgent, agent_target('4')),
    ];
    for (experience, target) in proven {
        try_profile(experience, target).unwrap_or_else(|err| {
            panic!(
                "{} authoring claim path must succeed: {err}",
                experience.as_str()
            )
        });
    }
}

#[test]
fn chatflow_and_workflow_fail_closed_on_preset_claim_path() {
    let denied = [
        (ApplicationExperience::Chatflow, model_target('5')),
        (ApplicationExperience::Workflow, model_target('6')),
    ];
    for (experience, target) in denied {
        let err = try_profile(experience, target).expect_err("must fail closed");
        assert!(
            err.contains("Chatflow and Workflow"),
            "{} must fail closed on preset authoring claim path, got {err}",
            experience.as_str()
        );
    }
}


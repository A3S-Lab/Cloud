use crate::modules::developer_workflows::{
    AcceptBuildPlanResult, AcceptedBuildPlan, BuildPlanDetection, BuildPlanDetectionDiagnostic,
    BuildPlanProposal, BUILD_PLAN_PROPOSAL_SCHEMA,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DetectBuildPlansRequest {
    pub source_revision_id: Uuid,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptBuildPlanRequest {
    pub source_revision_id: Uuid,
    pub proposal_acl: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPlanSourceResponse {
    pub source_identity_digest: String,
    pub commit_sha: String,
    pub source_content_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPlanRecipeResponse {
    pub schema: String,
    pub kind: String,
    pub context_path: String,
    pub dockerfile_path: String,
    pub target: Option<String>,
    pub platforms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPlanProposalResponse {
    pub schema: String,
    pub proposal_acl: String,
    pub proposal_digest: String,
    pub detector: String,
    pub detector_revision: String,
    pub project_root: String,
    pub evidence_path: String,
    pub evidence_digest: String,
    pub recipe: BuildPlanRecipeResponse,
}

impl From<BuildPlanProposal> for BuildPlanProposalResponse {
    fn from(proposal: BuildPlanProposal) -> Self {
        let spec = proposal.spec();
        Self {
            schema: BUILD_PLAN_PROPOSAL_SCHEMA.into(),
            proposal_acl: proposal.canonical_acl().into(),
            proposal_digest: proposal.digest().as_str().into(),
            detector: spec.detector.as_str().into(),
            detector_revision: spec.detector_revision.clone(),
            project_root: spec.project_root.clone(),
            evidence_path: spec.evidence_path.clone(),
            evidence_digest: spec.evidence_digest.as_str().into(),
            recipe: BuildPlanRecipeResponse {
                schema: spec.recipe.schema().into(),
                kind: spec.recipe.kind().into(),
                context_path: spec.recipe.context_path().into(),
                dockerfile_path: spec.recipe.dockerfile_path().into(),
                target: spec.recipe.target().map(str::to_owned),
                platforms: spec
                    .recipe
                    .platforms()
                    .iter()
                    .map(|platform| platform.as_str().to_owned())
                    .collect(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPlanDetectionDiagnosticResponse {
    pub code: String,
    pub path: Option<String>,
}

impl From<BuildPlanDetectionDiagnostic> for BuildPlanDetectionDiagnosticResponse {
    fn from(diagnostic: BuildPlanDetectionDiagnostic) -> Self {
        Self {
            code: diagnostic.code.as_str().into(),
            path: diagnostic.path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPlanDetectionResponse {
    pub source: BuildPlanSourceResponse,
    pub proposals: Vec<BuildPlanProposalResponse>,
    pub diagnostics: Vec<BuildPlanDetectionDiagnosticResponse>,
}

impl From<BuildPlanDetection> for BuildPlanDetectionResponse {
    fn from(detection: BuildPlanDetection) -> Self {
        Self {
            source: BuildPlanSourceResponse {
                source_identity_digest: detection.source.source_identity_digest.as_str().into(),
                commit_sha: detection.source.commit_sha.as_str().into(),
                source_content_digest: detection.source.content_digest.as_str().into(),
            },
            proposals: detection.proposals.into_iter().map(Into::into).collect(),
            diagnostics: detection.diagnostics.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedBuildPlanResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub build_plan_id: Uuid,
    pub source_revision_id: Uuid,
    pub contract_schema: String,
    pub contract_acl: String,
    pub contract_digest: String,
    pub proposal: BuildPlanProposalResponse,
    pub aggregate_version: u64,
    pub accepted_by: Uuid,
    pub accepted_at: DateTime<Utc>,
}

impl From<AcceptedBuildPlan> for AcceptedBuildPlanResponse {
    fn from(plan: AcceptedBuildPlan) -> Self {
        Self {
            organization_id: plan.organization_id.as_uuid(),
            project_id: plan.project_id.as_uuid(),
            environment_id: plan.environment_id.as_uuid(),
            build_plan_id: plan.id.as_uuid(),
            source_revision_id: plan.source_revision_id.as_uuid(),
            contract_schema: plan.contract.schema().into(),
            contract_acl: plan.contract.canonical_acl().into(),
            contract_digest: plan.contract.digest().as_str().into(),
            proposal: plan.contract.spec().proposal.clone().into(),
            aggregate_version: plan.aggregate_version,
            accepted_by: plan.accepted_by.as_uuid(),
            accepted_at: plan.accepted_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPlanMutationResponse {
    pub build_plan: AcceptedBuildPlanResponse,
    pub replayed: bool,
}

impl From<AcceptBuildPlanResult> for BuildPlanMutationResponse {
    fn from(result: AcceptBuildPlanResult) -> Self {
        Self {
            build_plan: result.plan.into(),
            replayed: result.replayed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::AcceptedBuildPlanContract;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, OrganizationId, PrincipalId, ProjectId, SourceRevisionId,
    };

    const BUILD_PLAN_FIXTURE: &str =
        include_str!("../../../../../../contracts/p0.1/build-plan.acl");

    #[test]
    fn build_plan_requests_are_closed_and_accept_only_acl_input() {
        let source_revision_id = Uuid::now_v7();
        let detection: DetectBuildPlansRequest = serde_json::from_value(serde_json::json!({
            "sourceRevisionId": source_revision_id
        }))
        .expect("closed detection request");
        assert_eq!(detection.source_revision_id, source_revision_id);

        let acceptance: AcceptBuildPlanRequest = serde_json::from_value(serde_json::json!({
            "sourceRevisionId": source_revision_id,
            "proposalAcl": BUILD_PLAN_FIXTURE
        }))
        .expect("closed acceptance request");
        assert_eq!(acceptance.source_revision_id, source_revision_id);
        assert_eq!(acceptance.proposal_acl, BUILD_PLAN_FIXTURE);

        assert!(
            serde_json::from_value::<DetectBuildPlansRequest>(serde_json::json!({
                "sourceRevisionId": source_revision_id,
                "sourceBytes": []
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<AcceptBuildPlanRequest>(serde_json::json!({
                "sourceRevisionId": source_revision_id,
                "proposalAcl": BUILD_PLAN_FIXTURE,
                "proposal": {}
            }))
            .is_err()
        );
    }

    #[test]
    fn accepted_build_plan_response_preserves_canonical_acl_and_typed_evidence() {
        let proposal = BuildPlanProposal::parse_acl(BUILD_PLAN_FIXTURE).expect("proposal fixture");
        let expected_proposal_digest = proposal.digest().as_str().to_owned();
        let source_revision_id = SourceRevisionId::new();
        let contract = AcceptedBuildPlanContract::from_proposal(source_revision_id, proposal)
            .expect("accepted contract");
        let expected_contract_acl = contract.canonical_acl().to_owned();
        let expected_contract_digest = contract.digest().as_str().to_owned();
        let plan = AcceptedBuildPlan::accept(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            contract,
            PrincipalId::new(),
            Utc::now(),
        )
        .expect("accepted BuildPlan");

        let response = AcceptedBuildPlanResponse::from(plan);
        assert_eq!(response.source_revision_id, source_revision_id.as_uuid());
        assert_eq!(response.contract_acl, expected_contract_acl);
        assert_eq!(response.contract_digest, expected_contract_digest);
        assert_eq!(response.proposal.proposal_acl, BUILD_PLAN_FIXTURE);
        assert_eq!(response.proposal.proposal_digest, expected_proposal_digest);
        assert_eq!(response.proposal.recipe.kind, "dockerfile");
        assert_eq!(
            response.proposal.recipe.platforms,
            vec!["linux/amd64".to_owned()]
        );

        let json = serde_json::to_value(response).expect("response JSON");
        assert!(json.get("contractAcl").is_some());
        assert!(json["proposal"].get("proposalAcl").is_some());
        assert!(json.get("checkoutPath").is_none());
        assert!(json.get("credentials").is_none());
    }
}

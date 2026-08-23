use crate::modules::developer_workflows::domain::{
    BuildPlanDetectionDiagnostic, BuildPlanDetectionDiagnosticCode, BuildPlanDetectorKind,
    BuildPlanDetectorOutput, BuildPlanProposal, BuildPlanProposalSpec, IBuildPlanDetector,
    SourceLayoutEntryKind, SourceLayoutSnapshot, BUILD_PLAN_DETECTOR_REVISION,
    MAX_BUILD_PLAN_PROPOSALS,
};
use crate::modules::sources::domain::BuildRecipe;

#[derive(Debug, Default)]
pub struct DockerfileBuildPlanDetector;

impl IBuildPlanDetector for DockerfileBuildPlanDetector {
    fn kind(&self) -> BuildPlanDetectorKind {
        BuildPlanDetectorKind::Dockerfile
    }

    fn detect(&self, layout: &SourceLayoutSnapshot) -> Result<BuildPlanDetectorOutput, String> {
        let candidates = layout
            .entries()
            .iter()
            .filter(|entry| {
                entry.kind() == SourceLayoutEntryKind::Regular
                    && entry.path().rsplit('/').next() == Some("Dockerfile")
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Ok(BuildPlanDetectorOutput::not_applicable());
        }
        if candidates.len() > MAX_BUILD_PLAN_PROPOSALS {
            return Err("Dockerfile detector exceeded the proposal bound".into());
        }
        let mut proposals = Vec::with_capacity(candidates.len());
        let mut diagnostics = Vec::new();
        for entry in candidates {
            if entry.size_bytes() == 0 {
                diagnostics.push(BuildPlanDetectionDiagnostic::new(
                    BuildPlanDetectionDiagnosticCode::EmptyDockerfile,
                    Some(entry.path().into()),
                )?);
                continue;
            }
            let project_root = super::super::domain::source_layout::parent_root(entry.path());
            let recipe = BuildRecipe::dockerfile(
                BuildRecipe::SCHEMA,
                BuildRecipe::DOCKERFILE_KIND,
                &project_root,
                entry.path(),
                None,
                vec!["linux/amd64".into()],
            )?;
            proposals.push(BuildPlanProposal::from_spec(BuildPlanProposalSpec {
                source: layout.identity().clone(),
                detector: self.kind(),
                detector_revision: BUILD_PLAN_DETECTOR_REVISION.into(),
                project_root,
                evidence_path: entry.path().into(),
                evidence_digest: entry.content_digest().clone(),
                recipe,
            })?);
        }
        Ok(BuildPlanDetectorOutput::heuristic(proposals, diagnostics))
    }
}

use crate::modules::assets::domain::{
    AssetManifestDefinition, ASSET_MANIFEST_MAX_ACL_BYTES, ASSET_MANIFEST_PATH,
};
use crate::modules::developer_workflows::domain::{
    BuildPlanDetectionDiagnostic, BuildPlanDetectionDiagnosticCode, BuildPlanDetectorKind,
    BuildPlanDetectorOutput, BuildPlanProposal, BuildPlanProposalSpec, IBuildPlanDetector,
    SourceLayoutEntryKind, SourceLayoutSnapshot, BUILD_PLAN_DETECTOR_REVISION,
};

#[derive(Debug, Default)]
pub struct AssetAclBuildPlanDetector;

impl IBuildPlanDetector for AssetAclBuildPlanDetector {
    fn kind(&self) -> BuildPlanDetectorKind {
        BuildPlanDetectorKind::AssetAcl
    }

    fn detect(&self, layout: &SourceLayoutSnapshot) -> Result<BuildPlanDetectorOutput, String> {
        let Some(entry) = layout.entry(ASSET_MANIFEST_PATH) else {
            return Ok(BuildPlanDetectorOutput::not_applicable());
        };
        if entry.kind() != SourceLayoutEntryKind::Regular
            || entry.size_bytes() == 0
            || entry.size_bytes() as usize > ASSET_MANIFEST_MAX_ACL_BYTES
        {
            return Err("explicit Asset ACL evidence is not a bounded regular file".into());
        }
        let content = entry
            .inspected_content()
            .ok_or_else(|| "explicit Asset ACL evidence was not inspected".to_owned())?;
        let source = std::str::from_utf8(content)
            .map_err(|_| "explicit Asset ACL evidence is not UTF-8".to_owned())?;
        let definition = AssetManifestDefinition::parse_acl(source)
            .map_err(|error| format!("explicit Asset ACL is invalid: {error}"))?;
        let Some(recipe) = definition.build_recipe else {
            return Ok(BuildPlanDetectorOutput::authoritative(
                Vec::new(),
                vec![BuildPlanDetectionDiagnostic::new(
                    BuildPlanDetectionDiagnosticCode::AssetBuildRecipeMissing,
                    Some(ASSET_MANIFEST_PATH.into()),
                )?],
            ));
        };
        let dockerfile = layout
            .entry(recipe.dockerfile_path())
            .ok_or_else(|| "Asset ACL build recipe references a missing Dockerfile".to_owned())?;
        if dockerfile.kind() != SourceLayoutEntryKind::Regular || dockerfile.size_bytes() == 0 {
            return Err("Asset ACL build recipe references an invalid Dockerfile".into());
        }
        let proposal = BuildPlanProposal::from_spec(BuildPlanProposalSpec {
            source: layout.identity().clone(),
            detector: self.kind(),
            detector_revision: BUILD_PLAN_DETECTOR_REVISION.into(),
            project_root: recipe.context_path().into(),
            evidence_path: ASSET_MANIFEST_PATH.into(),
            evidence_digest: entry.content_digest().clone(),
            recipe,
        })?;
        Ok(BuildPlanDetectorOutput::authoritative(
            vec![proposal],
            Vec::new(),
        ))
    }
}

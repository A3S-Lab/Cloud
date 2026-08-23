use crate::modules::developer_workflows::domain::{
    BuildPlanDetection, BuildPlanDetectionDiagnostic, BuildPlanDetectionDiagnosticCode,
    BuildPlanDetectorKind, BuildPlanDetectorMatch, BuildPlanDetectorOutput, IBuildPlanDetector,
    SourceLayoutEntryKind, SourceLayoutSnapshot, MAX_BUILD_PLAN_DETECTORS,
    MAX_BUILD_PLAN_PROPOSALS,
};
use std::collections::BTreeSet;
use std::sync::Arc;

pub struct BuildPlanDetectionService {
    detectors: Vec<Arc<dyn IBuildPlanDetector>>,
}

impl BuildPlanDetectionService {
    pub fn new(mut detectors: Vec<Arc<dyn IBuildPlanDetector>>) -> Result<Self, String> {
        if detectors.is_empty() || detectors.len() > MAX_BUILD_PLAN_DETECTORS {
            return Err("BuildPlan detector set must be bounded and non-empty".into());
        }
        detectors.sort_by_key(|detector| detector.kind());
        let kinds = detectors
            .iter()
            .map(|detector| detector.kind())
            .collect::<BTreeSet<_>>();
        if kinds.len() != detectors.len() {
            return Err("BuildPlan detector kinds must be unique".into());
        }
        Ok(Self { detectors })
    }

    pub fn detect(&self, layout: &SourceLayoutSnapshot) -> Result<BuildPlanDetection, String> {
        layout.validate()?;
        let mut proposals = Vec::new();
        let mut diagnostics = Vec::new();
        for detector in &self.detectors {
            let output = detector.detect(layout)?;
            self.validate_output(detector.kind(), layout, &output)?;
            match output.matched {
                BuildPlanDetectorMatch::NotApplicable => {}
                BuildPlanDetectorMatch::Heuristic => {
                    proposals.extend(output.proposals);
                    diagnostics.extend(output.diagnostics);
                }
                BuildPlanDetectorMatch::Authoritative => {
                    proposals = output.proposals;
                    diagnostics = output.diagnostics;
                    break;
                }
            }
        }
        if proposals.is_empty() && diagnostics.is_empty() {
            diagnostics.push(BuildPlanDetectionDiagnostic::new(
                BuildPlanDetectionDiagnosticCode::NoSupportedLayout,
                None,
            )?);
        }
        if proposals.len() > MAX_BUILD_PLAN_PROPOSALS {
            return Err("BuildPlan detection exceeded the proposal bound".into());
        }
        proposals.sort_by(|left, right| {
            left.spec()
                .project_root
                .cmp(&right.spec().project_root)
                .then_with(|| left.spec().detector.cmp(&right.spec().detector))
                .then_with(|| left.digest().cmp(right.digest()))
        });
        if proposals
            .windows(2)
            .any(|pair| pair[0].spec().project_root == pair[1].spec().project_root)
        {
            return Err("BuildPlan detectors produced an ambiguous project root".into());
        }
        diagnostics.sort_by(|left, right| {
            left.code
                .cmp(&right.code)
                .then_with(|| left.path.cmp(&right.path))
        });
        diagnostics.dedup();
        let detection = BuildPlanDetection {
            source: layout.identity().clone(),
            proposals,
            diagnostics,
        };
        detection.validate()?;
        Ok(detection)
    }

    fn validate_output(
        &self,
        detector_kind: BuildPlanDetectorKind,
        layout: &SourceLayoutSnapshot,
        output: &BuildPlanDetectorOutput,
    ) -> Result<(), String> {
        if output.matched == BuildPlanDetectorMatch::NotApplicable
            && (!output.proposals.is_empty() || !output.diagnostics.is_empty())
        {
            return Err("non-applicable BuildPlan detector returned evidence".into());
        }
        if output.proposals.len() > MAX_BUILD_PLAN_PROPOSALS {
            return Err("BuildPlan detector exceeded the proposal bound".into());
        }
        for proposal in &output.proposals {
            proposal.validate()?;
            let spec = proposal.spec();
            if spec.detector != detector_kind || &spec.source != layout.identity() {
                return Err("BuildPlan detector changed its kind or source identity".into());
            }
            let evidence = layout
                .entry(&spec.evidence_path)
                .ok_or_else(|| "BuildPlan detector referenced missing evidence".to_owned())?;
            if evidence.kind() != SourceLayoutEntryKind::Regular
                || evidence.content_digest() != &spec.evidence_digest
            {
                return Err("BuildPlan detector evidence does not match the source layout".into());
            }
        }
        for diagnostic in &output.diagnostics {
            BuildPlanDetectionDiagnostic::new(diagnostic.code, diagnostic.path.clone())?;
            if diagnostic
                .path
                .as_deref()
                .is_some_and(|path| layout.entry(path).is_none())
            {
                return Err("BuildPlan diagnostic referenced missing source evidence".into());
            }
        }
        Ok(())
    }
}

use super::{BuildPlanDetectorKind, BuildPlanProposal, SourceLayoutIdentity, SourceLayoutSnapshot};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeSet;

pub const MAX_BUILD_PLAN_DETECTORS: usize = 8;
pub const MAX_BUILD_PLAN_PROPOSALS: usize = 16;
pub const MAX_BUILD_PLAN_DIAGNOSTICS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildPlanDetectorMatch {
    NotApplicable,
    Heuristic,
    Authoritative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildPlanDetectionDiagnosticCode {
    AssetBuildRecipeMissing,
    EmptyDockerfile,
    NoSupportedLayout,
}

impl BuildPlanDetectionDiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AssetBuildRecipeMissing => "asset_build_recipe_missing",
            Self::EmptyDockerfile => "empty_dockerfile",
            Self::NoSupportedLayout => "no_supported_layout",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildPlanDetectionDiagnostic {
    pub code: BuildPlanDetectionDiagnosticCode,
    pub path: Option<String>,
}

impl BuildPlanDetectionDiagnostic {
    pub fn new(
        code: BuildPlanDetectionDiagnosticCode,
        path: Option<String>,
    ) -> Result<Self, String> {
        if let Some(path) = &path {
            super::source_layout::validate_repository_file_path(path)?;
        }
        if code == BuildPlanDetectionDiagnosticCode::NoSupportedLayout && path.is_some() {
            return Err("no-supported-layout diagnostic cannot name a path".into());
        }
        Ok(Self { code, path })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlanDetectorOutput {
    pub matched: BuildPlanDetectorMatch,
    pub proposals: Vec<BuildPlanProposal>,
    pub diagnostics: Vec<BuildPlanDetectionDiagnostic>,
}

impl BuildPlanDetectorOutput {
    pub const fn not_applicable() -> Self {
        Self {
            matched: BuildPlanDetectorMatch::NotApplicable,
            proposals: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn heuristic(
        proposals: Vec<BuildPlanProposal>,
        diagnostics: Vec<BuildPlanDetectionDiagnostic>,
    ) -> Self {
        Self {
            matched: BuildPlanDetectorMatch::Heuristic,
            proposals,
            diagnostics,
        }
    }

    pub fn authoritative(
        proposals: Vec<BuildPlanProposal>,
        diagnostics: Vec<BuildPlanDetectionDiagnostic>,
    ) -> Self {
        Self {
            matched: BuildPlanDetectorMatch::Authoritative,
            proposals,
            diagnostics,
        }
    }
}

pub trait IBuildPlanDetector: Send + Sync {
    fn kind(&self) -> BuildPlanDetectorKind;

    fn detect(&self, layout: &SourceLayoutSnapshot) -> Result<BuildPlanDetectorOutput, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlanDetection {
    pub source: SourceLayoutIdentity,
    pub proposals: Vec<BuildPlanProposal>,
    pub diagnostics: Vec<BuildPlanDetectionDiagnostic>,
}

impl BuildPlanDetection {
    pub fn validate(&self) -> Result<(), String> {
        self.source.validate()?;
        if self.proposals.len() > MAX_BUILD_PLAN_PROPOSALS {
            return Err("BuildPlan detection contains too many proposals".into());
        }
        if self.diagnostics.len() > MAX_BUILD_PLAN_DIAGNOSTICS {
            return Err("BuildPlan detection contains too many diagnostics".into());
        }
        let mut roots = BTreeSet::new();
        for proposal in &self.proposals {
            proposal.validate()?;
            if proposal.spec().source != self.source {
                return Err("BuildPlan proposal changed its source layout identity".into());
            }
            if !roots.insert(proposal.spec().project_root.as_str()) {
                return Err("BuildPlan detection contains an ambiguous project root".into());
            }
        }
        if self
            .proposals
            .windows(2)
            .any(|pair| pair[0].canonical_cmp(&pair[1]) != Ordering::Less)
        {
            return Err("BuildPlan detection proposals are not canonical".into());
        }
        for diagnostic in &self.diagnostics {
            BuildPlanDetectionDiagnostic::new(diagnostic.code, diagnostic.path.clone())?;
        }
        if self.diagnostics.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("BuildPlan detection diagnostics are not canonical".into());
        }
        Ok(())
    }
}

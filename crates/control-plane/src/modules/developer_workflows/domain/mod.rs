mod build_plan;
mod detection;
pub(crate) mod source_layout;

pub use build_plan::{
    BuildPlanDetectorKind, BuildPlanProposal, BuildPlanProposalSpec, BUILD_PLAN_DETECTOR_REVISION,
    BUILD_PLAN_PROPOSAL_MAX_ACL_BYTES, BUILD_PLAN_PROPOSAL_SCHEMA,
};
pub use detection::{
    BuildPlanDetection, BuildPlanDetectionDiagnostic, BuildPlanDetectionDiagnosticCode,
    BuildPlanDetectorMatch, BuildPlanDetectorOutput, IBuildPlanDetector, MAX_BUILD_PLAN_DETECTORS,
    MAX_BUILD_PLAN_DIAGNOSTICS, MAX_BUILD_PLAN_PROPOSALS,
};
pub use source_layout::{
    SourceLayoutEntry, SourceLayoutEntryKind, SourceLayoutIdentity, SourceLayoutSnapshot,
    MAX_SOURCE_LAYOUT_CONTENT_BYTES, MAX_SOURCE_LAYOUT_ENTRIES,
    MAX_SOURCE_LAYOUT_INSPECTED_FILE_BYTES,
};

#[cfg(test)]
mod tests;

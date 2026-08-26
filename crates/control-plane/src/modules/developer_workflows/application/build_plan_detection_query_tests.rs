use super::{BuildPlanDetectionService, DetectBuildPlanProposals, DetectBuildPlanProposalsHandler};
use crate::modules::developer_workflows::domain::{
    BuildPlanDetectionDiagnosticCode, BuildPlanDetectorKind, SourceLayoutEntry,
    SourceLayoutIdentity, SourceLayoutSnapshot,
};
use crate::modules::developer_workflows::infrastructure::{
    AssetAclBuildPlanDetector, DockerfileBuildPlanDetector,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{GitCommitSha, Sha256Digest};
use a3s_boot::{CqrsContext, ModuleRef, QueryHandler};
use std::sync::Arc;

#[tokio::test]
async fn exact_asset_acl_is_authoritative_through_the_production_detector_set() {
    let manifest = concat!(
        "asset {\n",
        "  kind = \"agent\"\n",
        "  schema = \"a3s.cloud.asset.v1\"\n",
        "  build {\n",
        "    context = \".\"\n",
        "    file = \"Dockerfile\"\n",
        "    platforms = [\"linux/amd64\", \"linux/arm64\"]\n",
        "    target = \"release\"\n",
        "  }\n",
        "}\n",
    );
    let layout = layout(
        'a',
        vec![
            inspected("services/ignored/Dockerfile", b"FROM scratch\n"),
            inspected(".a3s/asset.acl", manifest.as_bytes()),
            inspected("Dockerfile", b"FROM scratch\n"),
        ],
    );
    let source = layout.identity().clone();

    let detection = handler()
        .execute(DetectBuildPlanProposals { layout }, context())
        .await
        .expect("CQRS result")
        .expect("BuildPlan detection");

    assert_eq!(detection.source, source);
    assert_eq!(detection.proposals.len(), 1);
    assert_eq!(
        detection.proposals[0].spec().detector,
        BuildPlanDetectorKind::AssetAcl
    );
    assert_eq!(
        detection.proposals[0].spec().recipe.target(),
        Some("release")
    );
    assert!(detection.diagnostics.is_empty());
}

#[tokio::test]
async fn dockerfile_fallback_and_unsupported_layout_are_closed_and_canonical() {
    let handler = handler();
    let detected = handler
        .execute(
            DetectBuildPlanProposals {
                layout: layout(
                    'b',
                    vec![
                        inspected("worker/Dockerfile", b"FROM scratch\n"),
                        inspected("Dockerfile", b"FROM scratch\n"),
                    ],
                ),
            },
            context(),
        )
        .await
        .expect("CQRS result")
        .expect("Dockerfile detection");
    assert_eq!(detected.proposals.len(), 2);
    assert_eq!(detected.proposals[0].spec().project_root, ".");
    assert_eq!(detected.proposals[1].spec().project_root, "worker");
    assert!(detected
        .proposals
        .iter()
        .all(|proposal| { proposal.spec().detector == BuildPlanDetectorKind::Dockerfile }));

    let unsupported = handler
        .execute(
            DetectBuildPlanProposals {
                layout: layout('c', vec![inspected("README.md", b"example\n")]),
            },
            context(),
        )
        .await
        .expect("CQRS result")
        .expect("closed unsupported-layout result");
    assert!(unsupported.proposals.is_empty());
    assert_eq!(unsupported.diagnostics.len(), 1);
    assert_eq!(
        unsupported.diagnostics[0].code,
        BuildPlanDetectionDiagnosticCode::NoSupportedLayout
    );
}

#[tokio::test]
async fn invalid_authoritative_acl_fails_closed_without_heuristic_fallback() {
    let error = handler()
        .execute(
            DetectBuildPlanProposals {
                layout: layout(
                    'd',
                    vec![
                        inspected(".a3s/asset.acl", b"asset { kind = \"agent\" }\n"),
                        inspected("Dockerfile", b"FROM scratch\n"),
                    ],
                ),
            },
            context(),
        )
        .await
        .expect("CQRS result")
        .expect_err("invalid explicit Asset ACL must fail closed");
    let ApplicationError::Invalid(message) = error else {
        panic!("invalid explicit Asset ACL must remain an input error");
    };
    assert!(message.contains("explicit Asset ACL is invalid"));
}

fn handler() -> DetectBuildPlanProposalsHandler {
    let detection = BuildPlanDetectionService::new(vec![
        Arc::new(AssetAclBuildPlanDetector),
        Arc::new(DockerfileBuildPlanDetector),
    ])
    .expect("production detector set");
    DetectBuildPlanProposalsHandler::new(Arc::new(detection))
}

fn layout(seed: char, entries: Vec<SourceLayoutEntry>) -> SourceLayoutSnapshot {
    SourceLayoutSnapshot::new(
        SourceLayoutIdentity::new(
            digest(seed),
            GitCommitSha::parse(seed.to_string().repeat(40)).expect("commit SHA"),
            digest(seed),
        )
        .expect("source identity"),
        entries,
    )
    .expect("source layout")
}

fn inspected(path: &str, content: &[u8]) -> SourceLayoutEntry {
    SourceLayoutEntry::inspected_regular(path, content).expect("source entry")
}

fn digest(seed: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", seed.to_string().repeat(64))).expect("digest")
}

fn context() -> CqrsContext {
    CqrsContext::new(ModuleRef::new())
}

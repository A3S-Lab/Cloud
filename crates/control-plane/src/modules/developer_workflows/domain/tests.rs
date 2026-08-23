use super::*;
use crate::modules::developer_workflows::infrastructure::{
    AssetAclBuildPlanDetector, DockerfileBuildPlanDetector,
};
use crate::modules::developer_workflows::BuildPlanDetectionService;
use crate::modules::shared_kernel::domain::{GitCommitSha, Sha256Digest};
use std::sync::Arc;

const BUILD_PLAN_FIXTURE: &str = include_str!("../../../../../../contracts/p0.1/build-plan.acl");

#[test]
fn checked_in_build_plan_contract_is_canonical_and_closed() {
    let proposal = BuildPlanProposal::parse_acl(BUILD_PLAN_FIXTURE).expect("BuildPlan fixture");
    assert_eq!(proposal.canonical_acl(), BUILD_PLAN_FIXTURE);
    proposal.validate().expect("valid BuildPlan proposal");

    let unknown = BUILD_PLAN_FIXTURE.replace(
        "  schema = \"a3s.cloud.build-plan-proposal.v1\"\n",
        concat!(
            "  schema = \"a3s.cloud.build-plan-proposal.v1\"\n",
            "  unknown = true\n",
        ),
    );
    assert!(BuildPlanProposal::parse_acl(&unknown).is_err());
    assert!(BuildPlanProposal::parse_acl(&BUILD_PLAN_FIXTURE.replace(
        "detector_revision = \"p0.1-c1\"",
        "detector_revision = \"p0.1-c2\"",
    ))
    .is_err());
}

#[test]
fn dockerfile_detection_is_order_and_checkout_directory_independent() {
    let first = layout(
        'a',
        vec![
            inspected("services/worker/Dockerfile", b"FROM scratch\n"),
            inspected("README.md", b"example\n"),
            inspected("Dockerfile", b"FROM scratch\n"),
        ],
    );
    let second = layout(
        'a',
        vec![
            inspected("Dockerfile", b"FROM scratch\n"),
            inspected("README.md", b"example\n"),
            inspected("services/worker/Dockerfile", b"FROM scratch\n"),
        ],
    );
    let detector = service();
    let first = detector.detect(&first).expect("first detection");
    let second = detector.detect(&second).expect("second detection");

    assert_eq!(first, second);
    assert_eq!(first.proposals.len(), 2);
    assert_eq!(first.proposals[0].spec().project_root, ".");
    assert_eq!(first.proposals[1].spec().project_root, "services/worker");
    assert!(first.diagnostics.is_empty());
    assert!(first.proposals.iter().all(|proposal| {
        proposal.spec().detector == BuildPlanDetectorKind::Dockerfile
            && proposal.spec().recipe.platforms()[0].as_str() == "linux/amd64"
    }));
}

#[test]
fn explicit_asset_acl_is_authoritative_and_uses_the_assets_owned_parser() {
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
    let detection = service()
        .detect(&layout(
            'b',
            vec![
                inspected("services/ignored/Dockerfile", b"FROM scratch\n"),
                inspected(".a3s/asset.acl", manifest.as_bytes()),
                inspected("Dockerfile", b"FROM scratch\n"),
            ],
        ))
        .expect("Asset detection");

    assert_eq!(detection.proposals.len(), 1);
    let proposal = &detection.proposals[0];
    assert_eq!(proposal.spec().detector, BuildPlanDetectorKind::AssetAcl);
    assert_eq!(proposal.spec().recipe.target(), Some("release"));
    assert_eq!(proposal.spec().recipe.platforms().len(), 2);
    assert_eq!(proposal.spec().evidence_path, ".a3s/asset.acl");
}

#[test]
fn explicit_asset_acl_never_falls_back_to_heuristics() {
    let no_build = concat!(
        "asset {\n",
        "  kind = \"skill\"\n",
        "  schema = \"a3s.cloud.asset.v1\"\n",
        "}\n",
    );
    let detection = service()
        .detect(&layout(
            'c',
            vec![
                inspected(".a3s/asset.acl", no_build.as_bytes()),
                inspected("Dockerfile", b"FROM scratch\n"),
            ],
        ))
        .expect("authoritative no-build result");
    assert!(detection.proposals.is_empty());
    assert_eq!(
        detection.diagnostics,
        vec![BuildPlanDetectionDiagnostic {
            code: BuildPlanDetectionDiagnosticCode::AssetBuildRecipeMissing,
            path: Some(".a3s/asset.acl".into()),
        }]
    );

    let invalid = layout(
        'd',
        vec![
            inspected(".a3s/asset.acl", b"asset { kind = \"agent\" }\n"),
            inspected("Dockerfile", b"FROM scratch\n"),
        ],
    );
    assert!(service().detect(&invalid).is_err());
}

#[test]
fn empty_and_unsupported_layouts_return_closed_diagnostics() {
    let empty_dockerfile = service()
        .detect(&layout('e', vec![inspected("Dockerfile", b"")]))
        .expect("empty Dockerfile diagnostic");
    assert!(empty_dockerfile.proposals.is_empty());
    assert_eq!(
        empty_dockerfile.diagnostics[0].code,
        BuildPlanDetectionDiagnosticCode::EmptyDockerfile
    );

    let unsupported = service()
        .detect(&layout('f', vec![inspected("README.md", b"example\n")]))
        .expect("unsupported layout diagnostic");
    assert_eq!(
        unsupported.diagnostics,
        vec![BuildPlanDetectionDiagnostic {
            code: BuildPlanDetectionDiagnosticCode::NoSupportedLayout,
            path: None,
        }]
    );
}

#[test]
fn detection_rejects_overflow_instead_of_truncating() {
    let entries = (0..=MAX_BUILD_PLAN_PROPOSALS)
        .map(|index| inspected(&format!("service-{index}/Dockerfile"), b"FROM scratch\n"))
        .collect();
    assert!(service().detect(&layout('1', entries)).is_err());
}

#[test]
fn source_layout_is_canonical_and_plan_digest_binds_the_exact_tree() {
    assert!(SourceLayoutEntry::inspected_regular("../Dockerfile", b"FROM scratch\n").is_err());
    let duplicate = SourceLayoutSnapshot::new(
        identity('2'),
        vec![
            inspected("Dockerfile", b"FROM scratch\n"),
            inspected("Dockerfile", b"FROM scratch\n"),
        ],
    );
    assert!(duplicate.is_err());

    let first = service()
        .detect(&layout(
            '3',
            vec![inspected("Dockerfile", b"FROM scratch\n")],
        ))
        .expect("first tree");
    let second = service()
        .detect(&layout(
            '4',
            vec![inspected("Dockerfile", b"FROM scratch\n")],
        ))
        .expect("second tree");
    assert_ne!(first.proposals[0].digest(), second.proposals[0].digest());
}

fn service() -> BuildPlanDetectionService {
    BuildPlanDetectionService::new(vec![
        Arc::new(DockerfileBuildPlanDetector),
        Arc::new(AssetAclBuildPlanDetector),
    ])
    .expect("built-in detector set")
}

fn layout(seed: char, entries: Vec<SourceLayoutEntry>) -> SourceLayoutSnapshot {
    SourceLayoutSnapshot::new(identity(seed), entries).expect("source layout")
}

fn identity(seed: char) -> SourceLayoutIdentity {
    SourceLayoutIdentity::new(
        digest(seed),
        GitCommitSha::parse("1".repeat(40)).expect("commit SHA"),
        digest(seed),
    )
    .expect("source identity")
}

fn digest(seed: char) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", seed.to_string().repeat(64))).expect("digest")
}

fn inspected(path: &str, contents: &[u8]) -> SourceLayoutEntry {
    SourceLayoutEntry::inspected_regular(path, contents).expect("source entry")
}

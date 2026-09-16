use super::*;
use crate::modules::shared_kernel::domain::{
    ExternalKnowledgeBindingId, KnowledgeBaseId, KnowledgeBaseRevisionId, KnowledgeChunkId,
    KnowledgeDatasourceEntranceId, KnowledgeDocumentId, KnowledgeIndexRevisionId, KnowledgePipelineId, KnowledgePipelineReleaseId,
    KnowledgeRetrievalPolicyRevisionId, OrganizationId, ProjectId, Sha256Digest, UserFileId,
    WorkflowDefinitionId, WorkflowRevisionId,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

const BASE_FIXTURE: &str =
    include_str!("../../../../../../contracts/k0.1/knowledge-base-revision.acl");
const DOCUMENT_FIXTURE: &str =
    include_str!("../../../../../../contracts/k0.1/knowledge-document.acl");
const CHUNK_FIXTURE: &str = include_str!("../../../../../../contracts/k0.1/knowledge-chunk.acl");
const INDEX_FIXTURE: &str =
    include_str!("../../../../../../contracts/k0.1/knowledge-index-revision.acl");
const POLICY_FIXTURE: &str =
    include_str!("../../../../../../contracts/k0.1/knowledge-retrieval-policy-revision.acl");
const BINDING_FIXTURE: &str =
    include_str!("../../../../../../contracts/k0.1/external-knowledge-binding.acl");
const PIPELINE_FIXTURE: &str =
    include_str!("../../../../../../contracts/k0.1/knowledge-pipeline-release.acl");
const ENTRANCE_FIXTURE: &str =
    include_str!("../../../../../../contracts/k0.2/knowledge-datasource-entrance.acl");

fn ts(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("timestamp")
        .with_timezone(&Utc)
}

fn id<T>(value: &str, ctor: impl FnOnce(Uuid) -> T) -> T {
    ctor(Uuid::parse_str(value).expect("uuid"))
}

fn digest(byte: u8) -> Sha256Digest {
    Sha256Digest::parse(format!("sha256:{}", format!("{byte:02x}").repeat(32))).expect("digest")
}

fn org() -> OrganizationId {
    id(
        "018f0000-0000-7000-8000-000000000201",
        OrganizationId::from_uuid,
    )
}
fn project() -> ProjectId {
    id("018f0000-0000-7000-8000-000000000202", ProjectId::from_uuid)
}

#[test]
fn knowledge_contracts_match_checked_in_fixtures_and_reject_drift() {
    let base = KnowledgeBaseRevisionV1::from_spec(KnowledgeBaseRevisionSpecV1 {
        organization_id: org(),
        project_id: project(),
        knowledge_base_id: id(
            "018f0000-0000-7000-8000-000000000301",
            KnowledgeBaseId::from_uuid,
        ),
        revision_id: id(
            "018f0000-0000-7000-8000-000000000302",
            KnowledgeBaseRevisionId::from_uuid,
        ),
        generation: 1,
        name: "Product FAQ".into(),
        chunk_structure: KnowledgeChunkStructureV1::General,
        retention_until: ts("2027-08-21T00:00:00Z"),
        tags: vec!["faq".into(), "public".into()],
        provenance_digest: digest(0xaa),
    })
    .expect("base revision");
    assert_eq!(base.canonical_acl(), BASE_FIXTURE);
    assert_eq!(
        KnowledgeBaseRevisionV1::parse_acl(BASE_FIXTURE).expect("parse"),
        base
    );
    assert!(KnowledgeBaseRevisionV1::parse_acl(BASE_FIXTURE.trim_end()).is_err());
    assert!(KnowledgeBaseRevisionV1::restore(BASE_FIXTURE, digest(0xbb).as_str()).is_err());

    let document = KnowledgeDocumentV1::from_spec(KnowledgeDocumentSpecV1 {
        organization_id: org(),
        project_id: project(),
        knowledge_base_id: base.spec().knowledge_base_id,
        knowledge_base_revision_id: base.spec().revision_id,
        document_id: id(
            "018f0000-0000-7000-8000-000000000303",
            KnowledgeDocumentId::from_uuid,
        ),
        title: "Return policy".into(),
        retention_until: ts("2027-08-21T00:00:00Z"),
        tags: vec!["returns".into()],
        provenance_digest: digest(0xcc),
        source: KnowledgeDocumentSourceV1::AdmittedUserFile {
            user_file_id: id(
                "018f0000-0000-7000-8000-000000000203",
                UserFileId::from_uuid,
            ),
            content_digest: digest(0x0a),
        },
    })
    .expect("document");
    assert_eq!(document.canonical_acl(), DOCUMENT_FIXTURE);
    assert_eq!(
        KnowledgeDocumentV1::parse_acl(DOCUMENT_FIXTURE).expect("parse"),
        document
    );

    let chunk = KnowledgeChunkV1::from_spec(KnowledgeChunkSpecV1 {
        organization_id: org(),
        project_id: project(),
        document_id: document.spec().document_id,
        chunk_id: id(
            "018f0000-0000-7000-8000-000000000304",
            KnowledgeChunkId::from_uuid,
        ),
        structure: KnowledgeChunkStructureV1::General,
        ordinal: 0,
        parent_chunk_id: None,
        content_digest: digest(0x0b),
        object_ref: "organizations/org/projects/proj/knowledge/chunks/0".into(),
        tags: vec!["section-1".into()],
        provenance_digest: digest(0xcd),
    })
    .expect("chunk");
    assert_eq!(chunk.canonical_acl(), CHUNK_FIXTURE);
    assert_eq!(
        KnowledgeChunkV1::parse_acl(CHUNK_FIXTURE).expect("parse"),
        chunk
    );
    let mut parented = chunk.spec().clone();
    parented.parent_chunk_id = Some(id(
        "018f0000-0000-7000-8000-000000000399",
        KnowledgeChunkId::from_uuid,
    ));
    assert!(KnowledgeChunkV1::from_spec(parented).is_err());

    let index = KnowledgeIndexRevisionV1::from_spec(KnowledgeIndexRevisionSpecV1 {
        organization_id: org(),
        project_id: project(),
        knowledge_base_revision_id: base.spec().revision_id,
        index_revision_id: id(
            "018f0000-0000-7000-8000-000000000305",
            KnowledgeIndexRevisionId::from_uuid,
        ),
        strategy: KnowledgeIndexStrategyV1::Hybrid,
        embedding_model_revision_digest: digest(0x11),
        embedding_dimension: 1536,
        input_modalities: vec!["text".into()],
        retrieval_modalities: vec!["text".into()],
    })
    .expect("index");
    assert_eq!(index.canonical_acl(), INDEX_FIXTURE);
    assert_eq!(
        KnowledgeIndexRevisionV1::parse_acl(INDEX_FIXTURE).expect("parse"),
        index
    );

    let policy = KnowledgeRetrievalPolicyRevisionV1::from_spec(
        KnowledgeRetrievalPolicyRevisionSpecV1 {
            organization_id: org(),
            project_id: project(),
            knowledge_base_revision_id: base.spec().revision_id,
            policy_revision_id: id(
                "018f0000-0000-7000-8000-000000000306",
                KnowledgeRetrievalPolicyRevisionId::from_uuid,
            ),
            search_mode: "hybrid".into(),
            filter_mode: "exact".into(),
            rerank_mode: "disabled".into(),
            score_mode: "standard".into(),
            citation_mode: "required".into(),
            top_k: 8,
        },
    )
    .expect("policy");
    assert_eq!(policy.canonical_acl(), POLICY_FIXTURE);
    assert_eq!(
        KnowledgeRetrievalPolicyRevisionV1::parse_acl(POLICY_FIXTURE).expect("parse"),
        policy
    );

    let binding = ExternalKnowledgeBindingV1::from_spec(ExternalKnowledgeBindingSpecV1 {
        organization_id: org(),
        project_id: project(),
        knowledge_base_id: base.spec().knowledge_base_id,
        binding_id: id(
            "018f0000-0000-7000-8000-000000000307",
            ExternalKnowledgeBindingId::from_uuid,
        ),
        display_name: "Partner corpus".into(),
        external_corpus_ref: "partner-corpus-v3".into(),
        external_corpus_digest: digest(0x22),
    })
    .expect("binding");
    assert_eq!(binding.canonical_acl(), BINDING_FIXTURE);
    assert_eq!(
        ExternalKnowledgeBindingV1::parse_acl(BINDING_FIXTURE).expect("parse"),
        binding
    );

    let pipeline = KnowledgePipelineReleaseV1::from_spec(KnowledgePipelineReleaseSpecV1 {
        organization_id: org(),
        project_id: project(),
        pipeline_id: id(
            "018f0000-0000-7000-8000-000000000308",
            KnowledgePipelineId::from_uuid,
        ),
        release_id: id(
            "018f0000-0000-7000-8000-000000000309",
            KnowledgePipelineReleaseId::from_uuid,
        ),
        name: "FAQ ingest".into(),
        chunk_structure: KnowledgeChunkStructureV1::General,
        workflow_definition_id: id(
            "018f0000-0000-7000-8000-000000000101",
            WorkflowDefinitionId::from_uuid,
        ),
        workflow_revision_id: id(
            "018f0000-0000-7000-8000-000000000102",
            WorkflowRevisionId::from_uuid,
        ),
        workflow_revision_digest: digest(0x33),
        datasource_entrance_digests: vec![digest(0x44)],
        output_contract_digest: digest(0x55),
    })
    .expect("pipeline");
    assert_eq!(pipeline.canonical_acl(), PIPELINE_FIXTURE);
    assert_eq!(
        KnowledgePipelineReleaseV1::parse_acl(PIPELINE_FIXTURE).expect("parse"),
        pipeline
    );
}


#[test]
fn k02_c1_file_text_datasource_entrance_matches_fixture_and_rejects_deferred_kinds() {
    let entrance = KnowledgeDatasourceEntranceV1::from_spec(KnowledgeDatasourceEntranceSpecV1 {
        organization_id: org(),
        project_id: project(),
        knowledge_base_id: id(
            "018f0000-0000-7000-8000-000000000301",
            KnowledgeBaseId::from_uuid,
        ),
        knowledge_base_revision_id: id(
            "018f0000-0000-7000-8000-000000000302",
            KnowledgeBaseRevisionId::from_uuid,
        ),
        entrance_id: id(
            "018f0000-0000-7000-8000-000000000401",
            KnowledgeDatasourceEntranceId::from_uuid,
        ),
        name: "FAQ upload".into(),
        provenance_digest: digest(0x44),
        kind: KnowledgeDatasourceEntranceKindV1::FileUpload {
            user_file_id: id(
                "018f0000-0000-7000-8000-000000000203",
                UserFileId::from_uuid,
            ),
            content_digest: digest(0x0a),
        },
    })
    .expect("entrance");
    assert_eq!(entrance.canonical_acl(), ENTRANCE_FIXTURE);
    assert_eq!(
        KnowledgeDatasourceEntranceV1::parse_acl(ENTRANCE_FIXTURE).expect("parse"),
        entrance
    );
    assert!(KnowledgeDatasourceEntranceV1::parse_acl(ENTRANCE_FIXTURE.trim_end()).is_err());
    assert!(
        KnowledgeDatasourceEntranceV1::restore(ENTRANCE_FIXTURE, digest(0xbb).as_str()).is_err()
    );

    let text = KnowledgeDatasourceEntranceV1::from_spec(KnowledgeDatasourceEntranceSpecV1 {
        organization_id: org(),
        project_id: project(),
        knowledge_base_id: id(
            "018f0000-0000-7000-8000-000000000301",
            KnowledgeBaseId::from_uuid,
        ),
        knowledge_base_revision_id: id(
            "018f0000-0000-7000-8000-000000000302",
            KnowledgeBaseRevisionId::from_uuid,
        ),
        entrance_id: id(
            "018f0000-0000-7000-8000-000000000402",
            KnowledgeDatasourceEntranceId::from_uuid,
        ),
        name: "FAQ paste".into(),
        provenance_digest: digest(0x45),
        kind: KnowledgeDatasourceEntranceKindV1::InlineText {
            content: KnowledgeContentReferenceV1 {
                object_ref: "organizations/org/projects/proj/knowledge/text/faq".into(),
                digest: digest(0x0c),
                size_bytes: 128,
                media_type: "text/plain".into(),
            },
        },
    })
    .expect("inline text");
    assert!(text.digest().as_str().starts_with("sha256:"));

    let bad_media = KnowledgeDatasourceEntranceV1::from_spec(KnowledgeDatasourceEntranceSpecV1 {
        organization_id: org(),
        project_id: project(),
        knowledge_base_id: id(
            "018f0000-0000-7000-8000-000000000301",
            KnowledgeBaseId::from_uuid,
        ),
        knowledge_base_revision_id: id(
            "018f0000-0000-7000-8000-000000000302",
            KnowledgeBaseRevisionId::from_uuid,
        ),
        entrance_id: id(
            "018f0000-0000-7000-8000-000000000403",
            KnowledgeDatasourceEntranceId::from_uuid,
        ),
        name: "bad media".into(),
        provenance_digest: digest(0x46),
        kind: KnowledgeDatasourceEntranceKindV1::InlineText {
            content: KnowledgeContentReferenceV1 {
                object_ref: "organizations/org/projects/proj/knowledge/text/bad".into(),
                digest: digest(0x0d),
                size_bytes: 32,
                media_type: "application/pdf".into(),
            },
        },
    });
    assert!(bad_media.is_err());

    let deferred = "knowledge_datasource_entrance {\n  entrance_id = \"018f0000-0000-7000-8000-000000000404\"\n  knowledge_base_id = \"018f0000-0000-7000-8000-000000000301\"\n  knowledge_base_revision_id = \"018f0000-0000-7000-8000-000000000302\"\n  name = \"crawl\"\n  organization_id = \"018f0000-0000-7000-8000-000000000201\"\n  project_id = \"018f0000-0000-7000-8000-000000000202\"\n  provenance_digest = \"sha256:4747474747474747474747474747474747474747474747474747474747474747\"\n  schema = \"cloud.knowledge-datasource-entrance.v1\"\n  kind {\n    name = \"web_crawler\"\n  }\n}\n";
    let err = KnowledgeDatasourceEntranceV1::parse_acl(deferred).expect_err("deferred");
    assert!(err.contains("deferred"), "{err}");
}

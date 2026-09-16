use super::*;
use crate::modules::shared_kernel::domain::{
    ExternalKnowledgeBindingId, KnowledgeBaseId, KnowledgeBaseRevisionId, KnowledgeChunkId,
    KnowledgeDatasourceEntranceId, KnowledgeDocumentId, KnowledgeIngestionProvenanceId, KnowledgeProcessorOutputContractId, KnowledgeDocumentIncrementalUpdateId, KnowledgeIngestionCancellationId, KnowledgeFailureCleanupId, KnowledgeSourceTombstoneId, KnowledgeIndexRevisionId, KnowledgePipelineId, KnowledgePipelineReleaseId,
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
const PROCESSOR_FIXTURE: &str =
    include_str!("../../../../../../contracts/k0.2/knowledge-processor-output-contract.acl");
const PROVENANCE_FIXTURE: &str =
    include_str!("../../../../../../contracts/k0.2/knowledge-ingestion-provenance.acl");
const TOMBSTONE_FIXTURE: &str =
    include_str!("../../../../../../contracts/k0.2/knowledge-source-tombstone.acl");
const INCREMENTAL_UPDATE_FIXTURE: &str =
    include_str!("../../../../../../contracts/k0.2/knowledge-document-incremental-update.acl");
const CANCELLATION_FIXTURE: &str =
    include_str!("../../../../../../contracts/k0.2/knowledge-ingestion-cancellation.acl");
const FAILURE_CLEANUP_FIXTURE: &str =
    include_str!("../../../../../../contracts/k0.2/knowledge-failure-cleanup.acl");

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

#[test]
fn k02_c2_builtin_plain_text_processor_output_matches_fixture_and_rejects_deferred_kinds() {
    let contract = KnowledgeProcessorOutputContractV1::from_spec(
        KnowledgeProcessorOutputContractSpecV1 {
            organization_id: org(),
            project_id: project(),
            contract_id: id(
                "018f0000-0000-7000-8000-000000000501",
                KnowledgeProcessorOutputContractId::from_uuid,
            ),
            name: "FAQ text extract".into(),
            kind: KnowledgeProcessorOutputContractKindV1::BuiltinPlainTextExtract {
                max_input_bytes: 1_048_576,
                output_media_type: "text/plain".into(),
            },
        },
    )
    .expect("contract");
    assert_eq!(contract.canonical_acl(), PROCESSOR_FIXTURE);
    assert_eq!(
        KnowledgeProcessorOutputContractV1::parse_acl(PROCESSOR_FIXTURE).expect("parse"),
        contract
    );
    assert!(KnowledgeProcessorOutputContractV1::parse_acl(PROCESSOR_FIXTURE.trim_end()).is_err());
    assert!(
        KnowledgeProcessorOutputContractV1::restore(PROCESSOR_FIXTURE, digest(0xbb).as_str())
            .is_err()
    );

    let bad_media = KnowledgeProcessorOutputContractV1::from_spec(
        KnowledgeProcessorOutputContractSpecV1 {
            organization_id: org(),
            project_id: project(),
            contract_id: id(
                "018f0000-0000-7000-8000-000000000502",
                KnowledgeProcessorOutputContractId::from_uuid,
            ),
            name: "bad media".into(),
            kind: KnowledgeProcessorOutputContractKindV1::BuiltinPlainTextExtract {
                max_input_bytes: 1024,
                output_media_type: "application/pdf".into(),
            },
        },
    );
    assert!(bad_media.is_err());

    let deferred = "knowledge_processor_output_contract {\n  contract_id = \"018f0000-0000-7000-8000-000000000503\"\n  name = \"ocr\"\n  organization_id = \"018f0000-0000-7000-8000-000000000201\"\n  project_id = \"018f0000-0000-7000-8000-000000000202\"\n  schema = \"cloud.knowledge-processor-output-contract.v1\"\n  kind {\n    name = \"ocr_layout\"\n  }\n}\n";
    let err = KnowledgeProcessorOutputContractV1::parse_acl(deferred).expect_err("deferred");
    assert!(err.contains("deferred"), "{err}");
}

#[test]
fn k02_c3_file_text_ingestion_provenance_matches_fixture_and_rejects_deferred_kinds() {
    let provenance = KnowledgeIngestionProvenanceV1::from_spec(KnowledgeIngestionProvenanceSpecV1 {
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
        provenance_id: id(
            "018f0000-0000-7000-8000-000000000601",
            KnowledgeIngestionProvenanceId::from_uuid,
        ),
        name: "FAQ upload provenance".into(),
        kind: KnowledgeIngestionProvenanceKindV1::FileUploadBuiltinText {
            user_file_id: id(
                "018f0000-0000-7000-8000-000000000203",
                UserFileId::from_uuid,
            ),
            entrance_digest: digest(0x11),
            processor_output_contract_digest: digest(0x22),
            admitted_content_digest: digest(0x0a),
        },
    })
    .expect("provenance");
    assert_eq!(provenance.canonical_acl(), PROVENANCE_FIXTURE);
    assert_eq!(
        KnowledgeIngestionProvenanceV1::parse_acl(PROVENANCE_FIXTURE).expect("parse"),
        provenance
    );
    assert!(KnowledgeIngestionProvenanceV1::parse_acl(PROVENANCE_FIXTURE.trim_end()).is_err());
    assert!(
        KnowledgeIngestionProvenanceV1::restore(PROVENANCE_FIXTURE, digest(0xbb).as_str()).is_err()
    );

    let inline = KnowledgeIngestionProvenanceV1::from_spec(KnowledgeIngestionProvenanceSpecV1 {
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
        provenance_id: id(
            "018f0000-0000-7000-8000-000000000602",
            KnowledgeIngestionProvenanceId::from_uuid,
        ),
        name: "FAQ paste provenance".into(),
        kind: KnowledgeIngestionProvenanceKindV1::InlineTextBuiltinText {
            entrance_digest: digest(0x33),
            processor_output_contract_digest: digest(0x44),
            admitted_content_digest: digest(0x0c),
        },
    })
    .expect("inline provenance");
    assert!(inline.digest().as_str().starts_with("sha256:"));

    let deferred = "knowledge_ingestion_provenance {\n  knowledge_base_id = \"018f0000-0000-7000-8000-000000000301\"\n  knowledge_base_revision_id = \"018f0000-0000-7000-8000-000000000302\"\n  name = \"crawl provenance\"\n  organization_id = \"018f0000-0000-7000-8000-000000000201\"\n  project_id = \"018f0000-0000-7000-8000-000000000202\"\n  provenance_id = \"018f0000-0000-7000-8000-000000000603\"\n  schema = \"cloud.knowledge-ingestion-provenance.v1\"\n  kind {\n    name = \"web_crawler\"\n  }\n}\n";
    let err = KnowledgeIngestionProvenanceV1::parse_acl(deferred).expect_err("deferred");
    assert!(err.contains("deferred"), "{err}");
}

#[test]
fn k02_c4_file_text_source_tombstone_matches_fixture_and_rejects_deferred_kinds() {
    let tombstone = KnowledgeSourceTombstoneV1::from_spec(KnowledgeSourceTombstoneSpecV1 {
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
        tombstone_id: id(
            "018f0000-0000-7000-8000-000000000701",
            KnowledgeSourceTombstoneId::from_uuid,
        ),
        name: "FAQ upload tombstone".into(),
        kind: KnowledgeSourceTombstoneKindV1::AdmittedUserFile {
            document_id: id(
                "018f0000-0000-7000-8000-000000000501",
                KnowledgeDocumentId::from_uuid,
            ),
            provenance_digest: digest(0xaa),
            user_file_id: id(
                "018f0000-0000-7000-8000-000000000203",
                UserFileId::from_uuid,
            ),
            content_digest: digest(0x0a),
        },
    })
    .expect("tombstone");
    assert_eq!(tombstone.canonical_acl(), TOMBSTONE_FIXTURE);
    assert_eq!(
        KnowledgeSourceTombstoneV1::parse_acl(TOMBSTONE_FIXTURE).expect("parse"),
        tombstone
    );
    assert!(KnowledgeSourceTombstoneV1::parse_acl(TOMBSTONE_FIXTURE.trim_end()).is_err());
    assert!(
        KnowledgeSourceTombstoneV1::restore(TOMBSTONE_FIXTURE, digest(0xbb).as_str()).is_err()
    );

    let inline = KnowledgeSourceTombstoneV1::from_spec(KnowledgeSourceTombstoneSpecV1 {
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
        tombstone_id: id(
            "018f0000-0000-7000-8000-000000000702",
            KnowledgeSourceTombstoneId::from_uuid,
        ),
        name: "FAQ paste tombstone".into(),
        kind: KnowledgeSourceTombstoneKindV1::ImmutableObject {
            document_id: id(
                "018f0000-0000-7000-8000-000000000502",
                KnowledgeDocumentId::from_uuid,
            ),
            provenance_digest: digest(0xab),
            content_digest: digest(0x0c),
        },
    })
    .expect("inline tombstone");
    assert!(inline.digest().as_str().starts_with("sha256:"));

    let deferred = "knowledge_source_tombstone {\n  knowledge_base_id = \"018f0000-0000-7000-8000-000000000301\"\n  knowledge_base_revision_id = \"018f0000-0000-7000-8000-000000000302\"\n  name = \"crawl tombstone\"\n  organization_id = \"018f0000-0000-7000-8000-000000000201\"\n  project_id = \"018f0000-0000-7000-8000-000000000202\"\n  schema = \"cloud.knowledge-source-tombstone.v1\"\n  tombstone_id = \"018f0000-0000-7000-8000-000000000703\"\n  kind {\n    name = \"web_crawler\"\n  }\n}\n";
    let err = KnowledgeSourceTombstoneV1::parse_acl(deferred).expect_err("deferred");
    assert!(err.contains("deferred"), "{err}");
}

#[test]
fn k02_c5_file_text_document_incremental_update_matches_fixture_and_rejects_noop_and_deferred() {
    let update = KnowledgeDocumentIncrementalUpdateV1::from_spec(KnowledgeDocumentIncrementalUpdateSpecV1 {
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
        update_id: id(
            "018f0000-0000-7000-8000-000000000801",
            KnowledgeDocumentIncrementalUpdateId::from_uuid,
        ),
        name: "FAQ upload incremental update".into(),
        kind: KnowledgeDocumentIncrementalUpdateKindV1::ReplaceAdmittedUserFile {
            document_id: id(
                "018f0000-0000-7000-8000-000000000501",
                KnowledgeDocumentId::from_uuid,
            ),
            previous_provenance_digest: digest(0xaa),
            next_provenance_digest: digest(0xab),
            previous_content_digest: digest(0x0a),
            next_content_digest: digest(0x0b),
            user_file_id: id(
                "018f0000-0000-7000-8000-000000000203",
                UserFileId::from_uuid,
            ),
        },
    })
    .expect("incremental update");
    assert_eq!(update.canonical_acl(), INCREMENTAL_UPDATE_FIXTURE);
    assert_eq!(
        KnowledgeDocumentIncrementalUpdateV1::parse_acl(INCREMENTAL_UPDATE_FIXTURE).expect("parse"),
        update
    );
    assert!(KnowledgeDocumentIncrementalUpdateV1::parse_acl(INCREMENTAL_UPDATE_FIXTURE.trim_end()).is_err());
    assert!(
        KnowledgeDocumentIncrementalUpdateV1::restore(INCREMENTAL_UPDATE_FIXTURE, digest(0xbb).as_str())
            .is_err()
    );

    let noop = KnowledgeDocumentIncrementalUpdateV1::from_spec(KnowledgeDocumentIncrementalUpdateSpecV1 {
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
        update_id: id(
            "018f0000-0000-7000-8000-000000000802",
            KnowledgeDocumentIncrementalUpdateId::from_uuid,
        ),
        name: "noop update".into(),
        kind: KnowledgeDocumentIncrementalUpdateKindV1::ReplaceImmutableObject {
            document_id: id(
                "018f0000-0000-7000-8000-000000000502",
                KnowledgeDocumentId::from_uuid,
            ),
            previous_provenance_digest: digest(0xaa),
            next_provenance_digest: digest(0xaa),
            previous_content_digest: digest(0x0c),
            next_content_digest: digest(0x0d),
        },
    })
    .expect_err("noop provenance");
    assert!(noop.contains("provenance_digest"), "{noop}");

    let inline = KnowledgeDocumentIncrementalUpdateV1::from_spec(KnowledgeDocumentIncrementalUpdateSpecV1 {
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
        update_id: id(
            "018f0000-0000-7000-8000-000000000803",
            KnowledgeDocumentIncrementalUpdateId::from_uuid,
        ),
        name: "FAQ paste incremental update".into(),
        kind: KnowledgeDocumentIncrementalUpdateKindV1::ReplaceImmutableObject {
            document_id: id(
                "018f0000-0000-7000-8000-000000000502",
                KnowledgeDocumentId::from_uuid,
            ),
            previous_provenance_digest: digest(0xaa),
            next_provenance_digest: digest(0xac),
            previous_content_digest: digest(0x0c),
            next_content_digest: digest(0x0d),
        },
    })
    .expect("inline incremental");
    assert!(inline.digest().as_str().starts_with("sha256:"));

    let deferred = "knowledge_document_incremental_update {\n  knowledge_base_id = \"018f0000-0000-7000-8000-000000000301\"\n  knowledge_base_revision_id = \"018f0000-0000-7000-8000-000000000302\"\n  name = \"crawl update\"\n  organization_id = \"018f0000-0000-7000-8000-000000000201\"\n  project_id = \"018f0000-0000-7000-8000-000000000202\"\n  schema = \"cloud.knowledge-document-incremental-update.v1\"\n  update_id = \"018f0000-0000-7000-8000-000000000804\"\n  kind {\n    name = \"web_crawler\"\n  }\n}\n";
    let err = KnowledgeDocumentIncrementalUpdateV1::parse_acl(deferred).expect_err("deferred");
    assert!(err.contains("deferred"), "{err}");
}

#[test]
fn k02_c6_file_text_ingestion_cancellation_matches_fixture_and_rejects_noop_and_deferred() {
    let cancellation = KnowledgeIngestionCancellationV1::from_spec(KnowledgeIngestionCancellationSpecV1 {
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
        cancellation_id: id(
            "018f0000-0000-7000-8000-000000000901",
            KnowledgeIngestionCancellationId::from_uuid,
        ),
        name: "FAQ upload cancellation".into(),
        kind: KnowledgeIngestionCancellationKindV1::CancelAdmittedUserFileIngestion {
            document_id: id(
                "018f0000-0000-7000-8000-000000000501",
                KnowledgeDocumentId::from_uuid,
            ),
            provenance_digest: digest(0xaa),
            content_digest: digest(0x0a),
            user_file_id: id(
                "018f0000-0000-7000-8000-000000000203",
                UserFileId::from_uuid,
            ),
        },
    })
    .expect("cancellation");
    assert_eq!(cancellation.canonical_acl(), CANCELLATION_FIXTURE);
    assert_eq!(
        KnowledgeIngestionCancellationV1::parse_acl(CANCELLATION_FIXTURE).expect("parse"),
        cancellation
    );
    assert!(KnowledgeIngestionCancellationV1::parse_acl(CANCELLATION_FIXTURE.trim_end()).is_err());
    assert!(
        KnowledgeIngestionCancellationV1::restore(CANCELLATION_FIXTURE, digest(0xbb).as_str())
            .is_err()
    );

    let noop = KnowledgeIngestionCancellationV1::from_spec(KnowledgeIngestionCancellationSpecV1 {
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
        cancellation_id: id(
            "018f0000-0000-7000-8000-000000000902",
            KnowledgeIngestionCancellationId::from_uuid,
        ),
        name: "noop update cancel".into(),
        kind: KnowledgeIngestionCancellationKindV1::CancelDocumentIncrementalUpdate {
            document_id: id(
                "018f0000-0000-7000-8000-000000000502",
                KnowledgeDocumentId::from_uuid,
            ),
            update_id: id(
                "018f0000-0000-7000-8000-000000000801",
                KnowledgeDocumentIncrementalUpdateId::from_uuid,
            ),
            previous_provenance_digest: digest(0xaa),
            next_provenance_digest: digest(0xaa),
        },
    })
    .expect_err("noop provenance");
    assert!(noop.contains("provenance_digest"), "{noop}");

    let inline = KnowledgeIngestionCancellationV1::from_spec(KnowledgeIngestionCancellationSpecV1 {
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
        cancellation_id: id(
            "018f0000-0000-7000-8000-000000000903",
            KnowledgeIngestionCancellationId::from_uuid,
        ),
        name: "FAQ paste cancellation".into(),
        kind: KnowledgeIngestionCancellationKindV1::CancelImmutableObjectIngestion {
            document_id: id(
                "018f0000-0000-7000-8000-000000000502",
                KnowledgeDocumentId::from_uuid,
            ),
            provenance_digest: digest(0xac),
            content_digest: digest(0x0c),
        },
    })
    .expect("inline cancellation");
    assert!(inline.digest().as_str().starts_with("sha256:"));

    let deferred = "knowledge_ingestion_cancellation {\n  cancellation_id = \"018f0000-0000-7000-8000-000000000904\"\n  knowledge_base_id = \"018f0000-0000-7000-8000-000000000301\"\n  knowledge_base_revision_id = \"018f0000-0000-7000-8000-000000000302\"\n  name = \"crawl cancel\"\n  organization_id = \"018f0000-0000-7000-8000-000000000201\"\n  project_id = \"018f0000-0000-7000-8000-000000000202\"\n  schema = \"cloud.knowledge-ingestion-cancellation.v1\"\n  kind {\n    name = \"web_crawler\"\n  }\n}\n";
    let err = KnowledgeIngestionCancellationV1::parse_acl(deferred).expect_err("deferred");
    assert!(err.contains("deferred"), "{err}");
}

#[test]
fn k02_c7_file_text_failure_cleanup_matches_fixture_and_rejects_noop_and_deferred() {
    let cleanup = KnowledgeFailureCleanupV1::from_spec(KnowledgeFailureCleanupSpecV1 {
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
        cleanup_id: id(
            "018f0000-0000-7000-8000-000000000a01",
            KnowledgeFailureCleanupId::from_uuid,
        ),
        name: "FAQ upload failure cleanup".into(),
        kind: KnowledgeFailureCleanupKindV1::FailedAdmittedUserFileIngestion {
            document_id: id(
                "018f0000-0000-7000-8000-000000000501",
                KnowledgeDocumentId::from_uuid,
            ),
            provenance_digest: digest(0xaa),
            content_digest: digest(0x0a),
            user_file_id: id(
                "018f0000-0000-7000-8000-000000000203",
                UserFileId::from_uuid,
            ),
            failure_digest: digest(0xee),
        },
    })
    .expect("cleanup");
    assert_eq!(cleanup.canonical_acl(), FAILURE_CLEANUP_FIXTURE);
    assert_eq!(
        KnowledgeFailureCleanupV1::parse_acl(FAILURE_CLEANUP_FIXTURE).expect("parse"),
        cleanup
    );
    assert!(KnowledgeFailureCleanupV1::parse_acl(FAILURE_CLEANUP_FIXTURE.trim_end()).is_err());
    assert!(
        KnowledgeFailureCleanupV1::restore(FAILURE_CLEANUP_FIXTURE, digest(0xbb).as_str()).is_err()
    );

    let noop = KnowledgeFailureCleanupV1::from_spec(KnowledgeFailureCleanupSpecV1 {
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
        cleanup_id: id(
            "018f0000-0000-7000-8000-000000000a02",
            KnowledgeFailureCleanupId::from_uuid,
        ),
        name: "noop update failure cleanup".into(),
        kind: KnowledgeFailureCleanupKindV1::FailedDocumentIncrementalUpdate {
            document_id: id(
                "018f0000-0000-7000-8000-000000000502",
                KnowledgeDocumentId::from_uuid,
            ),
            update_id: id(
                "018f0000-0000-7000-8000-000000000801",
                KnowledgeDocumentIncrementalUpdateId::from_uuid,
            ),
            previous_provenance_digest: digest(0xaa),
            next_provenance_digest: digest(0xaa),
            failure_digest: digest(0xef),
        },
    })
    .expect_err("noop provenance");
    assert!(noop.contains("provenance_digest"), "{noop}");

    let inline = KnowledgeFailureCleanupV1::from_spec(KnowledgeFailureCleanupSpecV1 {
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
        cleanup_id: id(
            "018f0000-0000-7000-8000-000000000a03",
            KnowledgeFailureCleanupId::from_uuid,
        ),
        name: "FAQ paste failure cleanup".into(),
        kind: KnowledgeFailureCleanupKindV1::FailedImmutableObjectIngestion {
            document_id: id(
                "018f0000-0000-7000-8000-000000000502",
                KnowledgeDocumentId::from_uuid,
            ),
            provenance_digest: digest(0xac),
            content_digest: digest(0x0c),
            failure_digest: digest(0xf0),
        },
    })
    .expect("inline cleanup");
    assert!(inline.digest().as_str().starts_with("sha256:"));

    let deferred = "knowledge_failure_cleanup {\n  cleanup_id = \"018f0000-0000-7000-8000-000000000a04\"\n  knowledge_base_id = \"018f0000-0000-7000-8000-000000000301\"\n  knowledge_base_revision_id = \"018f0000-0000-7000-8000-000000000302\"\n  name = \"crawl cleanup\"\n  organization_id = \"018f0000-0000-7000-8000-000000000201\"\n  project_id = \"018f0000-0000-7000-8000-000000000202\"\n  schema = \"cloud.knowledge-failure-cleanup.v1\"\n  kind {\n    name = \"web_crawler\"\n  }\n}\n";
    let err = KnowledgeFailureCleanupV1::parse_acl(deferred).expect_err("deferred");
    assert!(err.contains("deferred"), "{err}");
}

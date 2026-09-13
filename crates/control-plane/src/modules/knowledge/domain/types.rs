use serde::{Deserialize, Serialize};

pub const KNOWLEDGE_CONTRACT_MAX_ACL_BYTES: usize = 64 * 1024;
pub const KNOWLEDGE_BASE_REVISION_SCHEMA_V1: &str = "cloud.knowledge-base-revision.v1";
pub const KNOWLEDGE_DOCUMENT_SCHEMA_V1: &str = "cloud.knowledge-document.v1";
pub const KNOWLEDGE_CHUNK_SCHEMA_V1: &str = "cloud.knowledge-chunk.v1";
pub const KNOWLEDGE_INDEX_REVISION_SCHEMA_V1: &str = "cloud.knowledge-index-revision.v1";
pub const KNOWLEDGE_RETRIEVAL_POLICY_REVISION_SCHEMA_V1: &str =
    "cloud.knowledge-retrieval-policy-revision.v1";
pub const EXTERNAL_KNOWLEDGE_BINDING_SCHEMA_V1: &str = "cloud.external-knowledge-binding.v1";
pub const KNOWLEDGE_PIPELINE_RELEASE_SCHEMA_V1: &str = "cloud.knowledge-pipeline-release.v1";

pub const KNOWLEDGE_MAX_NAME_BYTES: usize = 63;
pub const KNOWLEDGE_MAX_TAG_BYTES: usize = 64;
pub const KNOWLEDGE_MAX_TAGS: usize = 32;
pub const KNOWLEDGE_MAX_TOP_K: u32 = 100;
pub const KNOWLEDGE_MAX_EMBEDDING_DIMENSION: u32 = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeChunkStructureV1 {
    General,
    ParentChild,
    Qa,
}

impl KnowledgeChunkStructureV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::ParentChild => "parent_child",
            Self::Qa => "qa",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "general" => Ok(Self::General),
            "parent_child" => Ok(Self::ParentChild),
            "qa" => Ok(Self::Qa),
            _ => Err("unsupported Knowledge chunk structure".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeIndexStrategyV1 {
    Vector,
    FullText,
    Hybrid,
    Inverted,
}

impl KnowledgeIndexStrategyV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Vector => "vector",
            Self::FullText => "full_text",
            Self::Hybrid => "hybrid",
            Self::Inverted => "inverted",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "vector" => Ok(Self::Vector),
            "full_text" => Ok(Self::FullText),
            "hybrid" => Ok(Self::Hybrid),
            "inverted" => Ok(Self::Inverted),
            _ => Err("unsupported Knowledge index strategy".into()),
        }
    }
}

use super::knowledge_operation::{
    is_base_collection_path, is_knowledge_path, is_pipeline_collection_path,
};

pub(super) fn component_description(name: &str) -> Option<&'static str> {
    match name {
        "KnowledgeBase" => Some(
            "Authoritative KnowledgeBase head projection binding the canonical revision ACL, generation, name, and digests.",
        ),
        "KnowledgeBaseList" => Some("Bounded list of authorized KnowledgeBase head projections."),
        "KnowledgeBaseMutation" => Some(
            "KnowledgeBase mutation result with explicit idempotent-replay state and the authoritative head projection.",
        ),
        "KnowledgePipeline" => Some(
            "Authoritative KnowledgePipeline head projection binding the canonical release ACL, name, and digests.",
        ),
        "KnowledgePipelineList" => {
            Some("Bounded list of authorized KnowledgePipeline head projections.")
        }
        "KnowledgePipelineMutation" => Some(
            "KnowledgePipeline mutation result with explicit idempotent-replay state and the authoritative head projection.",
        ),
        _ => None,
    }
}

pub(super) fn operation_summary(method: &str, path: &str) -> Option<&'static str> {
    if !is_knowledge_path(path) {
        return None;
    }
    match method {
        "post" if is_base_collection_path(path) => Some("Create a knowledge base"),
        "post" if path.ends_with("/revisions") => Some("Append a knowledge base revision"),
        "get" if is_base_collection_path(path) => Some("List knowledge bases"),
        "get" if path.contains("/knowledge-bases/") => Some("Get a knowledge base"),
        "post" if is_pipeline_collection_path(path) => Some("Create a knowledge pipeline"),
        "post" if path.ends_with("/releases") && path.contains("/knowledge-pipelines/") => {
            Some("Publish a knowledge pipeline release")
        }
        "get" if is_pipeline_collection_path(path) => Some("List knowledge pipelines"),
        "get" if path.contains("/knowledge-pipelines/") => Some("Get a knowledge pipeline"),
        _ => None,
    }
}

pub(super) fn operation_description(method: &str, path: &str) -> Option<&'static str> {
    if !is_knowledge_path(path) {
        return None;
    }
    match method {
        "post" if is_base_collection_path(path) => Some(
            "Creates one KnowledgeBase from a canonical A3S ACL revision contract. Audit, Outbox, and idempotency commit atomically through the authorized catalog lifecycle boundary.",
        ),
        "post" if path.ends_with("/revisions") => Some(
            "Appends one digest-fenced KnowledgeBase revision using optimistic concurrency against the current head digest.",
        ),
        "get" if is_base_collection_path(path) => Some(
            "Lists a bounded set of KnowledgeBase head projections after project authorization.",
        ),
        "get" if path.contains("/knowledge-bases/") => {
            Some("Reads one authorized KnowledgeBase head projection by immutable identity.")
        }
        "post" if is_pipeline_collection_path(path) => Some(
            "Creates one KnowledgePipeline from a canonical A3S ACL release contract. Audit, Outbox, and idempotency commit atomically through the authorized catalog lifecycle boundary.",
        ),
        "post" if path.ends_with("/releases") && path.contains("/knowledge-pipelines/") => Some(
            "Publishes one digest-fenced KnowledgePipeline release using optimistic concurrency against the current head digest.",
        ),
        "get" if is_pipeline_collection_path(path) => Some(
            "Lists a bounded set of KnowledgePipeline head projections after project authorization.",
        ),
        "get" if path.contains("/knowledge-pipelines/") => {
            Some("Reads one authorized KnowledgePipeline head projection by immutable identity.")
        }
        _ => None,
    }
}

pub(super) fn response_data_description(method: &str, path: &str) -> Option<&'static str> {
    if !is_knowledge_path(path) {
        return None;
    }
    match method {
        "post" if path.contains("/knowledge-pipelines") => Some(
            "The authoritative KnowledgePipeline head after the mutation plus an idempotent-replay indicator.",
        ),
        "post" => Some(
            "The authoritative KnowledgeBase head after the mutation plus an idempotent-replay indicator.",
        ),
        "get" if is_base_collection_path(path) => {
            Some("A bounded list of authorized KnowledgeBase head projections.")
        }
        "get" if is_pipeline_collection_path(path) => {
            Some("A bounded list of authorized KnowledgePipeline head projections.")
        }
        "get" if path.contains("/knowledge-pipelines/") => {
            Some("The authoritative KnowledgePipeline head projection.")
        }
        "get" => Some("The authoritative KnowledgeBase head projection."),
        _ => None,
    }
}

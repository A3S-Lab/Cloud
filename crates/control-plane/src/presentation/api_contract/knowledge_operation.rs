use crate::modules::knowledge::{
    DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT, DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT,
    KNOWLEDGE_BASE_COLLECTION_ROUTE, KNOWLEDGE_BASE_ITEM_ROUTE, KNOWLEDGE_BASE_REVISION_ROUTE,
    KNOWLEDGE_CONTROLLER_PREFIX, KNOWLEDGE_PIPELINE_COLLECTION_ROUTE,
    KNOWLEDGE_PIPELINE_ITEM_ROUTE, KNOWLEDGE_PIPELINE_RELEASE_ROUTE,
    MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT, MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT,
};
use serde_json::{Value, json};

pub(super) fn is_knowledge_path(path: &str) -> bool {
    is_base_collection_path(path)
        || is_base_item_path(path)
        || is_base_revision_path(path)
        || is_pipeline_collection_path(path)
        || is_pipeline_item_path(path)
        || is_pipeline_release_path(path)
}

pub(super) fn is_base_collection_path(path: &str) -> bool {
    path == full_route(KNOWLEDGE_BASE_COLLECTION_ROUTE)
}

fn is_base_item_path(path: &str) -> bool {
    path == full_route(KNOWLEDGE_BASE_ITEM_ROUTE)
}

fn is_base_revision_path(path: &str) -> bool {
    path == full_route(KNOWLEDGE_BASE_REVISION_ROUTE)
}

pub(super) fn is_pipeline_collection_path(path: &str) -> bool {
    path == full_route(KNOWLEDGE_PIPELINE_COLLECTION_ROUTE)
}

fn is_pipeline_item_path(path: &str) -> bool {
    path == full_route(KNOWLEDGE_PIPELINE_ITEM_ROUTE)
}

fn is_pipeline_release_path(path: &str) -> bool {
    path == full_route(KNOWLEDGE_PIPELINE_RELEASE_ROUTE)
}

pub(super) fn query_parameters(method: &str, path: &str) -> Vec<Value> {
    if method == "get" && is_base_collection_path(path) {
        vec![json!({
            "name": "limit",
            "in": "query",
            "required": false,
            "description": "Maximum KnowledgeBase head projections returned for the authorized project.",
            "schema": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_KNOWLEDGE_BASE_LIST_LIMIT,
                "default": DEFAULT_KNOWLEDGE_BASE_LIST_LIMIT
            }
        })]
    } else if method == "get" && is_pipeline_collection_path(path) {
        vec![json!({
            "name": "limit",
            "in": "query",
            "required": false,
            "description": "Maximum KnowledgePipeline head projections returned for the authorized project.",
            "schema": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_KNOWLEDGE_PIPELINE_LIST_LIMIT,
                "default": DEFAULT_KNOWLEDGE_PIPELINE_LIST_LIMIT
            }
        })]
    } else {
        Vec::new()
    }
}

pub(super) fn success_component(method: &str, path: &str, status: u16) -> Option<&'static str> {
    match (method, status) {
        ("get", 200) if is_base_collection_path(path) => Some("KnowledgeBaseListSuccess200"),
        ("get", 200) if is_base_item_path(path) => Some("KnowledgeBaseSuccess200"),
        ("post", 200 | 201) if is_base_collection_path(path) => Some(if status == 201 {
            "KnowledgeBaseMutationSuccess201"
        } else {
            "KnowledgeBaseMutationSuccess200"
        }),
        ("post", 200) if is_base_revision_path(path) => Some("KnowledgeBaseMutationSuccess200"),
        ("get", 200) if is_pipeline_collection_path(path) => {
            Some("KnowledgePipelineListSuccess200")
        }
        ("get", 200) if is_pipeline_item_path(path) => Some("KnowledgePipelineSuccess200"),
        ("post", 200 | 201) if is_pipeline_collection_path(path) => Some(if status == 201 {
            "KnowledgePipelineMutationSuccess201"
        } else {
            "KnowledgePipelineMutationSuccess200"
        }),
        ("post", 200 | 201) if is_pipeline_release_path(path) => Some(if status == 201 {
            "KnowledgePipelineMutationSuccess201"
        } else {
            "KnowledgePipelineMutationSuccess200"
        }),
        _ => None,
    }
}

fn full_route(route: &str) -> String {
    format!("{KNOWLEDGE_CONTROLLER_PREFIX}{route}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_routes_have_exact_bounded_contract_bindings() {
        let base_collection = full_route(KNOWLEDGE_BASE_COLLECTION_ROUTE);
        let base_item = full_route(KNOWLEDGE_BASE_ITEM_ROUTE);
        let pipeline_collection = full_route(KNOWLEDGE_PIPELINE_COLLECTION_ROUTE);
        assert!(is_knowledge_path(&base_collection));
        assert_eq!(query_parameters("get", &base_collection).len(), 1);
        assert_eq!(query_parameters("get", &pipeline_collection).len(), 1);
        assert_eq!(
            success_component("post", &base_collection, 201),
            Some("KnowledgeBaseMutationSuccess201")
        );
        assert_eq!(
            success_component("get", &base_item, 200),
            Some("KnowledgeBaseSuccess200")
        );
        assert_eq!(
            success_component("post", &pipeline_collection, 201),
            Some("KnowledgePipelineMutationSuccess201")
        );
    }
}

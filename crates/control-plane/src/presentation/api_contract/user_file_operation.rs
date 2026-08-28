use crate::modules::files::presentation::{
    USER_FILES_CONTROLLER_PREFIX, USER_FILE_COLLECTION_ROUTE, USER_FILE_ITEM_ROUTE,
    USER_FILE_QUOTA_ROUTE, USER_FILE_TOMBSTONE_ROUTE,
};
use crate::modules::files::{DEFAULT_USER_FILE_LIST_LIMIT, MAXIMUM_USER_FILE_LIST_LIMIT};
use serde_json::{json, Value};

pub(super) fn is_user_file_path(path: &str) -> bool {
    is_collection_path(path) || is_item_path(path) || is_tombstone_path(path) || is_quota_path(path)
}

pub(super) fn is_collection_path(path: &str) -> bool {
    path == full_route(USER_FILE_COLLECTION_ROUTE)
}

fn is_item_path(path: &str) -> bool {
    path == full_route(USER_FILE_ITEM_ROUTE)
}

fn is_tombstone_path(path: &str) -> bool {
    path == full_route(USER_FILE_TOMBSTONE_ROUTE)
}

fn is_quota_path(path: &str) -> bool {
    path == full_route(USER_FILE_QUOTA_ROUTE)
}

pub(super) fn query_parameters(method: &str, path: &str) -> Vec<Value> {
    if method == "get" && is_collection_path(path) {
        vec![json!({
            "name": "limit",
            "in": "query",
            "required": false,
            "description": "Maximum UserFile lifecycle projections returned for the authorized project.",
            "schema": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAXIMUM_USER_FILE_LIST_LIMIT,
                "default": DEFAULT_USER_FILE_LIST_LIMIT
            }
        })]
    } else {
        Vec::new()
    }
}

pub(super) fn success_component(method: &str, path: &str, status: u16) -> Option<&'static str> {
    match (method, status) {
        ("get", 200) if is_collection_path(path) => Some("UserFileListSuccess200"),
        ("get", 200) if is_item_path(path) => Some("UserFileSuccess200"),
        ("get", 200) if is_quota_path(path) => Some("UserFileQuotaSuccess200"),
        ("post", 200 | 201) if is_collection_path(path) => Some(if status == 201 {
            "UserFileMutationSuccess201"
        } else {
            "UserFileMutationSuccess200"
        }),
        ("post", 200) if is_tombstone_path(path) => Some("UserFileMutationSuccess200"),
        _ => None,
    }
}

fn full_route(route: &str) -> String {
    format!("{USER_FILES_CONTROLLER_PREFIX}{route}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_file_routes_have_exact_bounded_contract_bindings() {
        let collection = full_route(USER_FILE_COLLECTION_ROUTE);
        let item = full_route(USER_FILE_ITEM_ROUTE);
        let quota = full_route(USER_FILE_QUOTA_ROUTE);
        assert!(is_user_file_path(&collection));
        assert_eq!(query_parameters("get", &collection).len(), 1);
        assert_eq!(
            success_component("post", &collection, 201),
            Some("UserFileMutationSuccess201")
        );
        assert_eq!(
            success_component("get", &item, 200),
            Some("UserFileSuccess200")
        );
        assert_eq!(
            success_component("get", &quota, 200),
            Some("UserFileQuotaSuccess200")
        );
    }
}

use super::user_file_operation::{is_collection_path, is_user_file_path};

pub(super) fn component_description(name: &str) -> Option<&'static str> {
    match name {
        "UserFile" => Some(
            "Authoritative Files lifecycle projection binding canonical admission ACL, logical immutable-object evidence, scan admission, retention, and cleanup intent.",
        ),
        "UserFileList" => Some("Bounded list of authorized UserFile lifecycle projections."),
        "UserFileMutation" => Some(
            "UserFile mutation result with explicit idempotent-replay state and the authoritative aggregate projection.",
        ),
        "UserFileQuota" => Some(
            "Organization-wide Files quota ledger; allocated and available bytes are derived from the same transactional lifecycle authority.",
        ),
        _ => None,
    }
}

pub(super) fn operation_summary(method: &str, path: &str) -> Option<&'static str> {
    if !is_user_file_path(path) {
        return None;
    }
    match method {
        "post" if is_collection_path(path) => Some("Reserve a user file"),
        "post" if path.ends_with("/tombstone") => Some("Tombstone a user file"),
        "get" if is_collection_path(path) => Some("List user files"),
        "get" if path.ends_with("/user-file-quota") => Some("Get the user file quota"),
        "get" => Some("Get a user file"),
        _ => None,
    }
}

pub(super) fn operation_description(method: &str, path: &str) -> Option<&'static str> {
    if !is_user_file_path(path) {
        return None;
    }
    match method {
        "post" if is_collection_path(path) => Some(
            "Reserves one bounded UserFile from a canonical A3S ACL admission contract. Metadata, quota allocation, audit, Outbox, and idempotency commit atomically; byte transfer remains behind the internal streaming immutable-object port.",
        ),
        "post" if path.ends_with("/tombstone") => Some(
            "Tombstones one UserFile using optimistic concurrency. Any reserved quota is released in the same transaction and one lifecycle cleanup intent is emitted; no independent deletion queue is created.",
        ),
        "get" if is_collection_path(path) => Some(
            "Lists a bounded set of UserFile lifecycle projections after project authorization. Binary content, storage-provider details, and scanner configuration are never exposed.",
        ),
        "get" if path.ends_with("/user-file-quota") => Some(
            "Reads the organization-wide Files quota ledger. This endpoint requires organization-wide authorization and conceals the resource from restricted principals.",
        ),
        "get" => Some(
            "Reads one authorized UserFile lifecycle projection by immutable identity without returning binary content.",
        ),
        _ => None,
    }
}

pub(super) fn response_data_description(method: &str, path: &str) -> Option<&'static str> {
    if !is_user_file_path(path) {
        return None;
    }
    match method {
        "post" => Some(
            "The authoritative UserFile aggregate after the mutation plus an idempotent-replay indicator.",
        ),
        "get" if is_collection_path(path) => {
            Some("A bounded list of authorized UserFile lifecycle projections.")
        }
        "get" if path.ends_with("/user-file-quota") => Some(
            "The organization quota limit, transactional allocation, remaining availability, revision, and update time.",
        ),
        "get" => Some("The authoritative UserFile lifecycle projection."),
        _ => None,
    }
}

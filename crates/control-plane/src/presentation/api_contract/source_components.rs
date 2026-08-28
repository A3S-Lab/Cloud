use crate::modules::sources::domain::{GitReference, GitRepository};
use crate::modules::sources::published::BuildRecipe;
use crate::modules::sources::{
    MAXIMUM_GITHUB_SOURCE_DISCOVERY_CURSOR_BYTES, MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE,
};
use serde_json::{json, Map, Value};

pub(super) const SOURCE_DISCOVERY_SUCCESS_SCHEMA_BINDINGS: &[(&str, &str)] = &[
    (
        "GithubRepositoryDiscoveryPageSuccessResponse",
        "GithubRepositoryDiscoveryPage",
    ),
    (
        "GithubRepositoryReferenceDiscoveryPageSuccessResponse",
        "GithubRepositoryReferenceDiscoveryPage",
    ),
];

pub(super) const SOURCE_DISCOVERY_SUCCESS_RESPONSE_BINDINGS: &[(&str, u16, &str)] = &[
    (
        "GithubRepositoryDiscoveryPageSuccess200",
        200,
        "GithubRepositoryDiscoveryPageSuccessResponse",
    ),
    (
        "GithubRepositoryReferenceDiscoveryPageSuccess200",
        200,
        "GithubRepositoryReferenceDiscoveryPageSuccessResponse",
    ),
];

pub(super) fn install_source_discovery_component_schemas(schemas: &mut Map<String, Value>) {
    for (name, schema) in [
        ("GithubSourceRepository", github_source_repository_schema()),
        (
            "GithubDiscoveredRepository",
            github_discovered_repository_schema(),
        ),
        (
            "GithubRepositoryDiscoveryPage",
            github_repository_discovery_page_schema(),
        ),
        ("GithubDiscoveredBranch", github_discovered_branch_schema()),
        ("GithubDiscoveredTag", github_discovered_tag_schema()),
        (
            "GithubDiscoveredReference",
            github_discovered_reference_schema(),
        ),
        (
            "GithubRepositoryReferenceDiscoveryPage",
            github_repository_reference_discovery_page_schema(),
        ),
    ] {
        schemas.insert(name.into(), schema);
    }
}

pub(super) fn build_recipe_request_schema() -> Value {
    build_recipe_schema(false)
}

pub(super) fn build_recipe_response_schema() -> Value {
    build_recipe_schema(true)
}

fn build_recipe_schema(target_required: bool) -> Value {
    let required = if target_required {
        vec![
            "schema",
            "kind",
            "contextPath",
            "dockerfilePath",
            "target",
            "platforms",
        ]
    } else {
        vec![
            "schema",
            "kind",
            "contextPath",
            "dockerfilePath",
            "platforms",
        ]
    };
    object_schema(
        &required,
        json!({
            "schema": { "type": "string", "enum": [BuildRecipe::SCHEMA] },
            "kind": { "type": "string", "enum": [BuildRecipe::DOCKERFILE_KIND] },
            "contextPath": build_recipe_path_schema(true),
            "dockerfilePath": build_recipe_path_schema(false),
            "target": {
                "type": "string",
                "minLength": 1,
                "maxLength": BuildRecipe::MAX_TARGET_BYTES,
                "pattern": BuildRecipe::TARGET_PATTERN,
                "nullable": true
            },
            "platforms": {
                "type": "array",
                "minItems": 1,
                "maxItems": BuildRecipe::MAX_PLATFORMS,
                "uniqueItems": true,
                "x-a3s-canonical-order": "lexical-wire-value",
                "items": {
                    "type": "string",
                    "enum": BuildRecipe::SUPPORTED_PLATFORMS
                }
            }
        }),
    )
}

fn github_source_repository_schema() -> Value {
    object_schema(
        &["provider", "canonicalUrl", "identity"],
        json!({
            "provider": { "type": "string", "enum": ["github"] },
            "canonicalUrl": github_repository_url_schema(),
            "identity": {
                "type": "string",
                "minLength": 1,
                "maxLength": GitRepository::MAX_IDENTITY_BYTES,
                "pattern": GitRepository::github_identity_pattern()
            }
        }),
    )
}

pub(super) fn github_repository_url_schema() -> Value {
    json!({
        "type": "string",
        "format": "uri",
        "minLength": 1,
        "maxLength": GitRepository::MAX_CANONICAL_URL_BYTES,
        "x-a3s-max-utf8-bytes": GitRepository::MAX_CANONICAL_URL_BYTES,
        "pattern": GitRepository::github_canonical_url_pattern()
    })
}

fn github_discovered_repository_schema() -> Value {
    object_schema(
        &[
            "repository",
            "defaultBranch",
            "private",
            "fork",
            "archived",
            "disabled",
        ],
        json!({
            "repository": schema_ref("GithubSourceRepository"),
            "defaultBranch": named_git_reference_schema(),
            "private": { "type": "boolean" },
            "fork": { "type": "boolean" },
            "archived": { "type": "boolean" },
            "disabled": { "type": "boolean" }
        }),
    )
}

fn github_repository_discovery_page_schema() -> Value {
    object_schema(
        &["repositories", "nextCursor"],
        json!({
            "repositories": {
                "type": "array",
                "maxItems": MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE,
                "uniqueItems": true,
                "items": schema_ref("GithubDiscoveredRepository")
            },
            "nextCursor": discovery_cursor_schema()
        }),
    )
}

fn github_discovered_branch_schema() -> Value {
    discovered_reference_variant_schema("branch", json!({ "type": "boolean" }))
}

fn github_discovered_tag_schema() -> Value {
    discovered_reference_variant_schema(
        "tag",
        json!({ "type": "boolean", "nullable": true, "enum": [null] }),
    )
}

fn discovered_reference_variant_schema(kind: &str, protected: Value) -> Value {
    object_schema(
        &["kind", "name", "commitSha", "protected"],
        json!({
            "kind": { "type": "string", "enum": [kind] },
            "name": named_git_reference_schema(),
            "commitSha": {
                "type": "string",
                "pattern": "^(?:[0-9a-f]{40}|[0-9a-f]{64})$"
            },
            "protected": protected
        }),
    )
}

fn github_discovered_reference_schema() -> Value {
    json!({
        "oneOf": [
            schema_ref("GithubDiscoveredBranch"),
            schema_ref("GithubDiscoveredTag")
        ],
        "discriminator": { "propertyName": "kind" }
    })
}

fn github_repository_reference_discovery_page_schema() -> Value {
    object_schema(
        &["repository", "kind", "references", "nextCursor"],
        json!({
            "repository": schema_ref("GithubSourceRepository"),
            "kind": { "type": "string", "enum": ["branch", "tag"] },
            "references": {
                "type": "array",
                "maxItems": MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE,
                "uniqueItems": true,
                "items": schema_ref("GithubDiscoveredReference")
            },
            "nextCursor": discovery_cursor_schema()
        }),
    )
}

fn named_git_reference_schema() -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": GitReference::MAX_NAMED_REFERENCE_BYTES,
        "x-a3s-max-utf8-bytes": GitReference::MAX_NAMED_REFERENCE_BYTES,
        "pattern": "^(?!refs/)(?!/)(?!.*\\.(?:/|$))(?!.*\\/$)(?!.*//)(?!.*\\.\\.)(?!.*(?:^|/)\\.)(?!.*\\.lock(?:/|$))[A-Za-z0-9_.\\/-]+$"
    })
}

fn discovery_cursor_schema() -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": MAXIMUM_GITHUB_SOURCE_DISCOVERY_CURSOR_BYTES,
        "nullable": true
    })
}

fn schema_ref(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

fn build_recipe_path_schema(allow_root: bool) -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": BuildRecipe::MAX_REPOSITORY_PATH_BYTES,
        "x-a3s-max-utf8-bytes": BuildRecipe::MAX_REPOSITORY_PATH_BYTES,
        "description": if allow_root {
            "Canonical relative POSIX repository path; '.' denotes the repository root."
        } else {
            "Canonical relative POSIX repository file path."
        }
    })
}

fn object_schema(required: &[&str], properties: Value) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_recipe_schemas_share_one_sources_contract_with_directional_requiredness() {
        let request = build_recipe_request_schema();
        let response = build_recipe_response_schema();
        let properties = &response["properties"];
        assert_eq!(request["additionalProperties"], false);
        assert_eq!(response["additionalProperties"], false);
        assert_eq!(request["properties"], response["properties"]);
        assert!(!request["required"]
            .as_array()
            .expect("request required fields")
            .contains(&json!("target")));
        assert!(response["required"]
            .as_array()
            .expect("response required fields")
            .contains(&json!("target")));
        assert_eq!(properties["schema"]["enum"], json!([BuildRecipe::SCHEMA]));
        assert_eq!(
            properties["contextPath"]["maxLength"],
            BuildRecipe::MAX_REPOSITORY_PATH_BYTES
        );
        assert_eq!(
            properties["target"]["maxLength"],
            BuildRecipe::MAX_TARGET_BYTES
        );
        assert_eq!(
            properties["platforms"]["maxItems"],
            BuildRecipe::MAX_PLATFORMS
        );
        assert_eq!(
            properties["platforms"]["items"]["enum"],
            json!(BuildRecipe::SUPPORTED_PLATFORMS)
        );
    }

    #[test]
    fn source_discovery_schemas_are_closed_bounded_and_secretless() {
        assert_eq!(SOURCE_DISCOVERY_SUCCESS_SCHEMA_BINDINGS.len(), 2);
        assert_eq!(SOURCE_DISCOVERY_SUCCESS_RESPONSE_BINDINGS.len(), 2);
        let mut schemas = Map::new();
        install_source_discovery_component_schemas(&mut schemas);
        assert_eq!(schemas.len(), 7);
        for name in [
            "GithubSourceRepository",
            "GithubDiscoveredRepository",
            "GithubRepositoryDiscoveryPage",
            "GithubDiscoveredBranch",
            "GithubDiscoveredTag",
            "GithubRepositoryReferenceDiscoveryPage",
        ] {
            assert_eq!(schemas[name]["additionalProperties"], false, "{name}");
        }
        assert_eq!(
            schemas["GithubRepositoryDiscoveryPage"]["properties"]["repositories"]["maxItems"],
            MAXIMUM_GITHUB_SOURCE_DISCOVERY_PAGE_SIZE
        );
        assert_eq!(
            schemas["GithubRepositoryReferenceDiscoveryPage"]["properties"]["nextCursor"]
                ["maxLength"],
            MAXIMUM_GITHUB_SOURCE_DISCOVERY_CURSOR_BYTES
        );
        assert_eq!(
            schemas["GithubDiscoveredTag"]["properties"]["protected"]["enum"],
            json!([null])
        );
        let encoded = Value::Object(schemas).to_string();
        for forbidden in ["token", "credential", "privateKey", "authorization"] {
            assert!(!encoded.contains(forbidden));
        }
    }
}

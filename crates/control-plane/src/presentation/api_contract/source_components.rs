use crate::modules::sources::published::BuildRecipe;
use serde_json::{json, Value};

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
}

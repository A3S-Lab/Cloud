use super::components::install_components;
use super::operation::describe_operation;
use super::route::openapi_info;
use super::{
    API_MAJOR_VERSION, API_PREFIX, HTTP_METHODS, MINIMUM_DEPRECATION_DAYS, OPENAPI_CONTRACT_VERSION,
};
use a3s_boot::{BootApplication, BootError, Result, AUTH_PUBLIC_METADATA};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub fn generate_openapi_contract(application: &BootApplication) -> Result<Value> {
    let mut document =
        serde_json::to_value(application.openapi(openapi_info())).map_err(|error| {
            BootError::Internal(format!("failed to serialize the OpenAPI contract: {error}"))
        })?;
    document["x-a3s-api-major-version"] = json!(API_MAJOR_VERSION);
    document["x-a3s-api-contract-version"] = json!(OPENAPI_CONTRACT_VERSION);
    document["x-a3s-minimum-deprecation-days"] = json!(MINIMUM_DEPRECATION_DAYS);

    let public_operations = public_operations(application);
    normalize_and_describe_paths(&mut document, &public_operations)?;
    install_components(&mut document)?;
    Ok(document)
}

fn public_operations(application: &BootApplication) -> BTreeSet<(String, String)> {
    application
        .routes()
        .iter()
        .filter(|route| !route.openapi().hidden)
        .filter(|route| route.metadata_value(AUTH_PUBLIC_METADATA) == Some(&Value::Bool(true)))
        .map(|route| {
            (
                normalize_route_path(route.path()),
                route.method().as_str().to_ascii_lowercase(),
            )
        })
        .collect()
}

fn normalize_and_describe_paths(
    document: &mut Value,
    public_operations: &BTreeSet<(String, String)>,
) -> Result<()> {
    let paths = document
        .get_mut("paths")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| BootError::Internal("generated OpenAPI document has no paths".into()))?;
    let generated = std::mem::take(paths);
    let mut normalized = Map::new();

    for (full_path, mut path_item) in generated {
        let path = strip_api_prefix(&full_path)?;
        let operations = path_item.as_object_mut().ok_or_else(|| {
            BootError::Internal(format!("OpenAPI path `{full_path}` is not an object"))
        })?;
        for method in HTTP_METHODS {
            let Some(operation) = operations.get_mut(method) else {
                continue;
            };
            let is_public = public_operations.contains(&(full_path.clone(), method.to_owned()));
            describe_operation(operation, method, &path, is_public)?;
        }
        normalized.insert(path, path_item);
    }

    document["paths"] = Value::Object(normalized);
    Ok(())
}

fn strip_api_prefix(path: &str) -> Result<String> {
    let stripped = path.strip_prefix(API_PREFIX).ok_or_else(|| {
        BootError::Internal(format!("public route `{path}` is outside `{API_PREFIX}`"))
    })?;
    Ok(if stripped.is_empty() {
        "/".into()
    } else {
        stripped.into()
    })
}

fn normalize_route_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            segment
                .strip_prefix("{*")
                .and_then(|value| value.strip_suffix('}'))
                .map(|value| format!("{{{value}}}"))
                .unwrap_or_else(|| segment.to_owned())
        })
        .collect::<Vec<_>>()
        .join("/")
}

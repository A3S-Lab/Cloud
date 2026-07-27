use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
use uuid::Uuid;

pub const MAXIMUM_LIST_LIMIT: usize = 200;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyArguments {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvironmentScopeArguments {
    pub project_id: Uuid,
    pub environment_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeArguments {
    pub node_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationListArguments {
    #[serde(
        default = "default_list_limit",
        deserialize_with = "deserialize_list_limit"
    )]
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadArguments {
    pub workload_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeploymentArguments {
    pub deployment_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteArguments {
    pub route_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildRunListArguments {
    pub project_id: Uuid,
    pub environment_id: Uuid,
    #[serde(
        default = "default_list_limit",
        deserialize_with = "deserialize_list_limit"
    )]
    pub limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildRunArguments {
    pub build_run_id: Uuid,
}

pub fn parse<T>(value: Value) -> std::result::Result<T, ()>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_value(value).map_err(|_| ())
}

pub fn parse_optional<T>(value: Value) -> std::result::Result<T, ()>
where
    T: serde::de::DeserializeOwned,
{
    parse(if value.is_null() { json!({}) } else { value })
}

const fn default_list_limit() -> usize {
    50
}

fn deserialize_list_limit<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    let limit = usize::deserialize(deserializer)?;
    if !(1..=MAXIMUM_LIST_LIMIT).contains(&limit) {
        return Err(D::Error::custom(format!(
            "limit must be between 1 and {MAXIMUM_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}

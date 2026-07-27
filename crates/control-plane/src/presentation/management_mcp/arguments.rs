use crate::modules::fleet::application::MAX_LOG_PAGE_SIZE;
use crate::presentation::parse_log_cursor;
use a3s_runtime::contract::RuntimeLogStream;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
use uuid::Uuid;

pub const MAXIMUM_LIST_LIMIT: usize = 200;
pub const MAXIMUM_LOG_LIMIT: u16 = MAX_LOG_PAGE_SIZE;
pub const DEFAULT_LOG_LIMIT: u16 = 100;

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadLogArguments {
    pub workload_id: Uuid,
    pub revision_id: Uuid,
    #[serde(
        default,
        rename = "cursor",
        deserialize_with = "deserialize_log_cursor"
    )]
    pub after_sequence: Option<u64>,
    #[serde(
        default = "default_log_limit",
        deserialize_with = "deserialize_log_limit"
    )]
    pub limit: u16,
    pub stream: Option<LogStreamArguments>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildRunLogArguments {
    pub build_run_id: Uuid,
    #[serde(
        default,
        rename = "cursor",
        deserialize_with = "deserialize_log_cursor"
    )]
    pub after_sequence: Option<u64>,
    #[serde(
        default = "default_log_limit",
        deserialize_with = "deserialize_log_limit"
    )]
    pub limit: u16,
    pub stream: Option<LogStreamArguments>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStreamArguments {
    Stdout,
    Stderr,
}

impl From<LogStreamArguments> for RuntimeLogStream {
    fn from(stream: LogStreamArguments) -> Self {
        match stream {
            LogStreamArguments::Stdout => Self::Stdout,
            LogStreamArguments::Stderr => Self::Stderr,
        }
    }
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

const fn default_log_limit() -> u16 {
    DEFAULT_LOG_LIMIT
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

fn deserialize_log_limit<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: Deserializer<'de>,
{
    let limit = u16::deserialize(deserializer)?;
    if !(1..=MAXIMUM_LOG_LIMIT).contains(&limit) {
        return Err(D::Error::custom(format!(
            "log limit must be between 1 and {MAXIMUM_LOG_LIMIT}"
        )));
    }
    Ok(limit)
}

fn deserialize_log_cursor<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let cursor = String::deserialize(deserializer)?;
    parse_log_cursor(&cursor)
        .map(Some)
        .ok_or_else(|| D::Error::custom("invalid log cursor"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_arguments_decode_bounded_rest_compatible_pages() {
        let workload = parse::<WorkloadLogArguments>(json!({
            "workloadId": Uuid::new_v4(),
            "revisionId": Uuid::new_v4(),
            "cursor": "v1:42",
            "limit": 256,
            "stream": "stderr"
        }))
        .expect("valid workload log arguments");
        assert_eq!(workload.after_sequence, Some(42));
        assert_eq!(workload.limit, MAXIMUM_LOG_LIMIT);
        assert!(matches!(workload.stream, Some(LogStreamArguments::Stderr)));

        let build = parse::<BuildRunLogArguments>(json!({
            "buildRunId": Uuid::new_v4()
        }))
        .expect("default build log arguments");
        assert_eq!(build.after_sequence, None);
        assert_eq!(build.limit, DEFAULT_LOG_LIMIT);
        assert!(build.stream.is_none());
    }

    #[test]
    fn log_arguments_reject_unbounded_or_noncanonical_inputs() {
        let build_run_id = Uuid::new_v4();
        for arguments in [
            json!({"buildRunId": build_run_id, "limit": 0}),
            json!({"buildRunId": build_run_id, "limit": 257}),
            json!({"buildRunId": build_run_id, "cursor": "42"}),
            json!({"buildRunId": build_run_id, "cursor": null}),
            json!({"buildRunId": build_run_id, "stream": "combined"}),
            json!({"buildRunId": build_run_id, "organizationId": Uuid::new_v4()}),
        ] {
            assert!(parse::<BuildRunLogArguments>(arguments).is_err());
        }
    }
}

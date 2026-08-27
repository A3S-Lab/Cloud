use super::developer_workflow_components::{
    canonical_acl_schema, object_schema, repository_path_schema, schema_ref,
};
use super::workflow_components::{digest_schema, timestamp_schema, uuid_schema};
use crate::modules::developer_workflows::{
    ScheduledTaskCatchUpPolicy, WorkloadProfileKind, MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT,
    MAX_SOURCE_LAYOUT_PATH_BYTES, MAX_WORKLOAD_ENVIRONMENT_VARIABLES,
    MAX_WORKLOAD_ENVIRONMENT_VARIABLE_NAME_BYTES, MAX_WORKLOAD_HEALTH_PATH_BYTES,
    MAX_WORKLOAD_PROCESS_ARGUMENTS, MAX_WORKLOAD_PROCESS_COMMANDS,
    MAX_WORKLOAD_PROCESS_VALUE_BYTES, MAX_WORKLOAD_PROFILE_EXECUTION_TIMEOUT_MS,
    MAX_WORKLOAD_PROFILE_NAME_BYTES, MAX_WORKLOAD_PROFILE_SAFE_INTEGER,
    MAX_WORKLOAD_RESOURCE_CPU_MILLIS, MAX_WORKLOAD_RESOURCE_EPHEMERAL_STORAGE_BYTES,
    MAX_WORKLOAD_RESOURCE_MEMORY_BYTES, MAX_WORKLOAD_RESOURCE_PIDS, MAX_WORKLOAD_SCHEDULE_ATTEMPTS,
    MAX_WORKLOAD_SCHEDULE_BACKOFF_MS, MAX_WORKLOAD_SCHEDULE_CONCURRENCY,
    MAX_WORKLOAD_SCHEDULE_EXPRESSION_BYTES, MAX_WORKLOAD_SCHEDULE_HISTORY_COUNT,
    MAX_WORKLOAD_SCHEDULE_HISTORY_DAYS, MAX_WORKLOAD_SCHEDULE_MISFIRE_GRACE_MS,
    MAX_WORKLOAD_SCHEDULE_TIMEZONE_NAME_BYTES, MAX_WORKLOAD_SECRET_BINDINGS,
    MAX_WORKLOAD_SECRET_NAME_BYTES, MAX_WORKLOAD_SERVICE_ENVIRONMENT_VALUE_BYTES,
    MAX_WORKLOAD_SERVICE_PORTS, MAX_WORKLOAD_SERVICE_PORT_NAME_BYTES,
    WORKLOAD_PROFILE_MAX_ACL_BYTES, WORKLOAD_PROFILE_SCHEMA,
};
use serde_json::{json, Map, Value};

const WORKLOAD_PROFILE_ACL_EXAMPLE: &str =
    include_str!("../../../../../contracts/p0.2/workload-profile.acl");

pub(super) const WORKLOAD_PROFILE_SUCCESS_SCHEMA_BINDINGS: &[(&str, &str)] = &[
    (
        "AcceptedWorkloadProfileRevisionSuccessResponse",
        "AcceptedWorkloadProfileRevision",
    ),
    (
        "AcceptedWorkloadProfileRevisionListSuccessResponse",
        "AcceptedWorkloadProfileRevisionList",
    ),
    (
        "WorkloadProfileMutationSuccessResponse",
        "WorkloadProfileMutation",
    ),
];

pub(super) const WORKLOAD_PROFILE_SUCCESS_RESPONSE_BINDINGS: &[(&str, u16, &str)] = &[
    (
        "AcceptedWorkloadProfileRevisionSuccess200",
        200,
        "AcceptedWorkloadProfileRevisionSuccessResponse",
    ),
    (
        "AcceptedWorkloadProfileRevisionListSuccess200",
        200,
        "AcceptedWorkloadProfileRevisionListSuccessResponse",
    ),
    (
        "WorkloadProfileMutationSuccess200",
        200,
        "WorkloadProfileMutationSuccessResponse",
    ),
    (
        "WorkloadProfileMutationSuccess201",
        201,
        "WorkloadProfileMutationSuccessResponse",
    ),
];

pub(super) fn install_workload_profile_component_schemas(schemas: &mut Map<String, Value>) {
    for (name, schema) in [
        ("WorkloadProcess", workload_process_schema()),
        (
            "WorkloadSecretEnvironmentTarget",
            workload_secret_environment_target_schema(),
        ),
        (
            "WorkloadSecretFileTarget",
            workload_secret_file_target_schema(),
        ),
        (
            "WorkloadSecretRegistryCredentialTarget",
            workload_secret_registry_credential_target_schema(),
        ),
        ("WorkloadSecretTarget", workload_secret_target_schema()),
        ("WorkloadSecretBinding", workload_secret_binding_schema()),
        (
            "WorkloadProfileResources",
            workload_profile_resources_schema(),
        ),
        ("WorkloadServicePort", workload_service_port_schema()),
        (
            "WorkloadHttpHealthCheck",
            workload_http_health_check_schema(),
        ),
        (
            "ScheduledTaskRetryPolicy",
            scheduled_task_retry_policy_schema(),
        ),
        (
            "ScheduledTaskHistoryPolicy",
            scheduled_task_history_policy_schema(),
        ),
        ("ScheduledTaskSchedule", scheduled_task_schedule_schema()),
        ("WorkloadProfileSpec", workload_profile_spec_schema()),
        (
            "AcceptedWorkloadProfileRevision",
            accepted_workload_profile_revision_schema(),
        ),
        (
            "AcceptedWorkloadProfileRevisionList",
            accepted_workload_profile_revision_list_schema(),
        ),
        (
            "WorkloadProfileMutation",
            workload_profile_mutation_schema(),
        ),
    ] {
        schemas.insert(name.into(), schema);
    }
}

pub(super) fn accept_workload_profile_request_schema() -> Value {
    object_schema(
        &["buildPlanId", "profileAcl"],
        json!({
            "buildPlanId": uuid_schema(),
            "profileAcl": canonical_acl_schema(
                WORKLOAD_PROFILE_MAX_ACL_BYTES,
                WORKLOAD_PROFILE_ACL_EXAMPLE,
            )
        }),
    )
}

fn workload_process_schema() -> Value {
    object_schema(
        &["command", "args", "workingDirectory", "environment"],
        json!({
            "command": {
                "type": "array",
                "maxItems": MAX_WORKLOAD_PROCESS_COMMANDS,
                "items": {
                    "type": "string",
                    "maxLength": MAX_WORKLOAD_PROCESS_VALUE_BYTES,
                    "x-a3s-max-utf8-bytes": MAX_WORKLOAD_PROCESS_VALUE_BYTES
                }
            },
            "args": {
                "type": "array",
                "maxItems": MAX_WORKLOAD_PROCESS_ARGUMENTS,
                "items": {
                    "type": "string",
                    "maxLength": MAX_WORKLOAD_PROCESS_VALUE_BYTES,
                    "x-a3s-max-utf8-bytes": MAX_WORKLOAD_PROCESS_VALUE_BYTES
                }
            },
            "workingDirectory": {
                "type": "string",
                "minLength": 1,
                "maxLength": MAX_WORKLOAD_PROCESS_VALUE_BYTES,
                "x-a3s-max-utf8-bytes": MAX_WORKLOAD_PROCESS_VALUE_BYTES,
                "nullable": true
            },
            "environment": {
                "type": "object",
                "maxProperties": MAX_WORKLOAD_ENVIRONMENT_VARIABLES,
                "propertyNames": {
                    "maxLength": MAX_WORKLOAD_ENVIRONMENT_VARIABLE_NAME_BYTES,
                    "pattern": "^[A-Z_][A-Z0-9_]{0,254}$"
                },
                "additionalProperties": {
                    "type": "string",
                    "maxLength": MAX_WORKLOAD_SERVICE_ENVIRONMENT_VALUE_BYTES,
                    "x-a3s-max-utf8-bytes": MAX_WORKLOAD_SERVICE_ENVIRONMENT_VALUE_BYTES
                }
            }
        }),
    )
}

fn workload_secret_environment_target_schema() -> Value {
    object_schema(
        &["kind", "variable"],
        json!({
            "kind": { "type": "string", "enum": ["environment"] },
            "variable": {
                "type": "string",
                "pattern": "^[A-Z_][A-Z0-9_]{0,254}$"
            }
        }),
    )
}

fn workload_secret_file_target_schema() -> Value {
    object_schema(
        &["kind", "path", "mode"],
        json!({
            "kind": { "type": "string", "enum": ["file"] },
            "path": {
                "type": "string",
                "maxLength": MAX_WORKLOAD_PROCESS_VALUE_BYTES,
                "x-a3s-max-utf8-bytes": MAX_WORKLOAD_PROCESS_VALUE_BYTES,
                "pattern": "^(?!.*//)(?!.*(?:^|/)\\.\\.?(?:/|$))/.+$"
            },
            "mode": { "type": "integer", "minimum": 1, "maximum": 511 }
        }),
    )
}

fn workload_secret_registry_credential_target_schema() -> Value {
    object_schema(
        &["kind"],
        json!({
            "kind": { "type": "string", "enum": ["registry_credential"] }
        }),
    )
}

fn workload_secret_target_schema() -> Value {
    json!({
        "oneOf": [
            schema_ref("WorkloadSecretEnvironmentTarget"),
            schema_ref("WorkloadSecretFileTarget"),
            schema_ref("WorkloadSecretRegistryCredentialTarget")
        ],
        "discriminator": {
            "propertyName": "kind",
            "mapping": {
                "environment": "#/components/schemas/WorkloadSecretEnvironmentTarget",
                "file": "#/components/schemas/WorkloadSecretFileTarget",
                "registry_credential": "#/components/schemas/WorkloadSecretRegistryCredentialTarget"
            }
        }
    })
}

fn workload_secret_binding_schema() -> Value {
    object_schema(
        &["name", "secretId", "version", "target"],
        json!({
            "name": {
                "type": "string",
                "minLength": 1,
                "maxLength": MAX_WORKLOAD_SECRET_NAME_BYTES,
                "pattern": "^[A-Za-z0-9._-]+$"
            },
            "secretId": uuid_schema(),
            "version": {
                "type": "integer",
                "format": "int64",
                "minimum": 1,
                "maximum": MAX_WORKLOAD_PROFILE_SAFE_INTEGER
            },
            "target": schema_ref("WorkloadSecretTarget")
        }),
    )
}

fn workload_profile_resources_schema() -> Value {
    object_schema(
        &[
            "cpuMillis",
            "memoryBytes",
            "pids",
            "ephemeralStorageBytes",
            "executionTimeoutMs",
        ],
        json!({
            "cpuMillis": {
                "type": "integer", "format": "int64", "minimum": 1,
                "maximum": MAX_WORKLOAD_RESOURCE_CPU_MILLIS
            },
            "memoryBytes": {
                "type": "integer", "format": "int64", "minimum": 1,
                "maximum": MAX_WORKLOAD_RESOURCE_MEMORY_BYTES
            },
            "pids": {
                "type": "integer", "format": "int32", "minimum": 1,
                "maximum": MAX_WORKLOAD_RESOURCE_PIDS
            },
            "ephemeralStorageBytes": {
                "type": "integer",
                "format": "int64",
                "minimum": 1,
                "maximum": MAX_WORKLOAD_RESOURCE_EPHEMERAL_STORAGE_BYTES,
                "nullable": true
            },
            "executionTimeoutMs": {
                "type": "integer",
                "format": "int64",
                "minimum": 1,
                "maximum": MAX_WORKLOAD_PROFILE_EXECUTION_TIMEOUT_MS,
                "nullable": true
            }
        }),
    )
}

fn workload_service_port_schema() -> Value {
    object_schema(
        &["name", "containerPort"],
        json!({
            "name": {
                "type": "string",
                "minLength": 1,
                "maxLength": MAX_WORKLOAD_SERVICE_PORT_NAME_BYTES,
                "pattern": "^[a-z0-9._-]+$"
            },
            "containerPort": { "type": "integer", "minimum": 1, "maximum": 65535 }
        }),
    )
}

fn workload_http_health_check_schema() -> Value {
    object_schema(
        &[
            "portName",
            "path",
            "intervalMs",
            "timeoutMs",
            "healthyThreshold",
            "unhealthyThreshold",
            "stabilizationWindowMs",
        ],
        json!({
            "portName": {
                "type": "string", "minLength": 1,
                "maxLength": MAX_WORKLOAD_SERVICE_PORT_NAME_BYTES
            },
            "path": {
                "type": "string", "minLength": 1,
                "maxLength": MAX_WORKLOAD_HEALTH_PATH_BYTES, "pattern": "^/"
            },
            "intervalMs": {
                "type": "integer", "format": "int64", "minimum": 1,
                "maximum": MAX_WORKLOAD_PROFILE_SAFE_INTEGER
            },
            "timeoutMs": {
                "type": "integer", "format": "int64", "minimum": 1,
                "maximum": MAX_WORKLOAD_PROFILE_SAFE_INTEGER
            },
            "healthyThreshold": { "type": "integer", "minimum": 1, "maximum": 65535 },
            "unhealthyThreshold": { "type": "integer", "minimum": 1, "maximum": 65535 },
            "stabilizationWindowMs": {
                "type": "integer", "format": "int64", "minimum": 1,
                "maximum": MAX_WORKLOAD_PROFILE_SAFE_INTEGER
            }
        }),
    )
}

fn scheduled_task_retry_policy_schema() -> Value {
    object_schema(
        &["maximumAttempts", "initialBackoffMs", "maximumBackoffMs"],
        json!({
            "maximumAttempts": {
                "type": "integer", "minimum": 1,
                "maximum": MAX_WORKLOAD_SCHEDULE_ATTEMPTS
            },
            "initialBackoffMs": {
                "type": "integer", "format": "int64", "minimum": 1,
                "maximum": MAX_WORKLOAD_SCHEDULE_BACKOFF_MS
            },
            "maximumBackoffMs": {
                "type": "integer", "format": "int64", "minimum": 1,
                "maximum": MAX_WORKLOAD_SCHEDULE_BACKOFF_MS
            }
        }),
    )
}

fn scheduled_task_history_policy_schema() -> Value {
    object_schema(
        &["successfulLimit", "failedLimit", "maximumAgeDays"],
        json!({
            "successfulLimit": {
                "type": "integer", "minimum": 0,
                "maximum": MAX_WORKLOAD_SCHEDULE_HISTORY_COUNT
            },
            "failedLimit": {
                "type": "integer", "minimum": 0,
                "maximum": MAX_WORKLOAD_SCHEDULE_HISTORY_COUNT
            },
            "maximumAgeDays": {
                "type": "integer", "minimum": 1,
                "maximum": MAX_WORKLOAD_SCHEDULE_HISTORY_DAYS
            }
        }),
    )
}

fn scheduled_task_schedule_schema() -> Value {
    object_schema(
        &[
            "expression",
            "timezone",
            "catchUp",
            "maximumConcurrency",
            "misfireGraceMs",
            "retry",
            "history",
        ],
        json!({
            "expression": {
                "type": "string", "minLength": 1,
                "maxLength": MAX_WORKLOAD_SCHEDULE_EXPRESSION_BYTES,
                "x-a3s-max-utf8-bytes": MAX_WORKLOAD_SCHEDULE_EXPRESSION_BYTES
            },
            "timezone": {
                "type": "string", "minLength": 1,
                "maxLength": MAX_WORKLOAD_SCHEDULE_TIMEZONE_NAME_BYTES,
                "x-a3s-max-utf8-bytes": MAX_WORKLOAD_SCHEDULE_TIMEZONE_NAME_BYTES
            },
            "catchUp": {
                "type": "string",
                "enum": [
                    ScheduledTaskCatchUpPolicy::Skip.as_str(),
                    ScheduledTaskCatchUpPolicy::Latest.as_str()
                ]
            },
            "maximumConcurrency": {
                "type": "integer", "minimum": 1,
                "maximum": MAX_WORKLOAD_SCHEDULE_CONCURRENCY
            },
            "misfireGraceMs": {
                "type": "integer", "format": "int64", "minimum": 1,
                "maximum": MAX_WORKLOAD_SCHEDULE_MISFIRE_GRACE_MS
            },
            "retry": schema_ref("ScheduledTaskRetryPolicy"),
            "history": schema_ref("ScheduledTaskHistoryPolicy")
        }),
    )
}

fn workload_profile_spec_schema() -> Value {
    object_schema(
        &[
            "name",
            "kind",
            "process",
            "secrets",
            "resources",
            "ports",
            "health",
            "publicPort",
            "schedule",
        ],
        json!({
            "name": {
                "type": "string",
                "minLength": 1,
                "maxLength": MAX_WORKLOAD_PROFILE_NAME_BYTES,
                "pattern": "^[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?$"
            },
            "kind": {
                "type": "string",
                "enum": [
                    WorkloadProfileKind::Web.as_str(),
                    WorkloadProfileKind::Worker.as_str(),
                    WorkloadProfileKind::ScheduledTask.as_str()
                ]
            },
            "process": schema_ref("WorkloadProcess"),
            "secrets": {
                "type": "array",
                "maxItems": MAX_WORKLOAD_SECRET_BINDINGS,
                "uniqueItems": true,
                "x-a3s-canonical-order": ["name"],
                "items": schema_ref("WorkloadSecretBinding")
            },
            "resources": schema_ref("WorkloadProfileResources"),
            "ports": {
                "type": "array",
                "maxItems": MAX_WORKLOAD_SERVICE_PORTS,
                "uniqueItems": true,
                "x-a3s-canonical-order": ["name"],
                "items": schema_ref("WorkloadServicePort")
            },
            "health": {
                "allOf": [schema_ref("WorkloadHttpHealthCheck")],
                "nullable": true
            },
            "publicPort": {
                "type": "string",
                "maxLength": MAX_WORKLOAD_SERVICE_PORT_NAME_BYTES,
                "nullable": true
            },
            "schedule": {
                "allOf": [schema_ref("ScheduledTaskSchedule")],
                "nullable": true
            }
        }),
    )
}

fn accepted_workload_profile_revision_schema() -> Value {
    object_schema(
        &[
            "organizationId",
            "projectId",
            "environmentId",
            "workloadProfileId",
            "workloadProfileRevisionId",
            "revisionNumber",
            "buildPlanId",
            "sourceRevisionId",
            "contractSchema",
            "contractAcl",
            "contractDigest",
            "buildPlanDigest",
            "projectRoot",
            "profile",
            "acceptedBy",
            "acceptedAt",
        ],
        json!({
            "organizationId": uuid_schema(),
            "projectId": uuid_schema(),
            "environmentId": uuid_schema(),
            "workloadProfileId": uuid_schema(),
            "workloadProfileRevisionId": uuid_schema(),
            "revisionNumber": {
                "type": "integer",
                "format": "int64",
                "minimum": 1,
                "maximum": MAX_WORKLOAD_PROFILE_SAFE_INTEGER
            },
            "buildPlanId": uuid_schema(),
            "sourceRevisionId": uuid_schema(),
            "contractSchema": { "type": "string", "enum": [WORKLOAD_PROFILE_SCHEMA] },
            "contractAcl": canonical_acl_schema(
                WORKLOAD_PROFILE_MAX_ACL_BYTES,
                WORKLOAD_PROFILE_ACL_EXAMPLE,
            ),
            "contractDigest": digest_schema(),
            "buildPlanDigest": digest_schema(),
            "projectRoot": repository_path_schema(MAX_SOURCE_LAYOUT_PATH_BYTES),
            "profile": schema_ref("WorkloadProfileSpec"),
            "acceptedBy": uuid_schema(),
            "acceptedAt": timestamp_schema()
        }),
    )
}

fn accepted_workload_profile_revision_list_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT,
        "uniqueItems": true,
        "x-a3s-canonical-order": ["revisionNumber", "workloadProfileRevisionId"],
        "items": schema_ref("AcceptedWorkloadProfileRevision")
    })
}

fn workload_profile_mutation_schema() -> Value {
    object_schema(
        &["workloadProfileRevision", "replayed"],
        json!({
            "workloadProfileRevision": schema_ref("AcceptedWorkloadProfileRevision"),
            "replayed": { "type": "boolean" }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_set_is_closed_typed_bounded_and_acl_only() {
        assert_eq!(WORKLOAD_PROFILE_SUCCESS_SCHEMA_BINDINGS.len(), 3);
        assert_eq!(WORKLOAD_PROFILE_SUCCESS_RESPONSE_BINDINGS.len(), 4);
        let mut schemas = Map::new();
        install_workload_profile_component_schemas(&mut schemas);
        assert_eq!(schemas.len(), 16);
        for name in [
            "WorkloadProcess",
            "WorkloadSecretEnvironmentTarget",
            "WorkloadSecretFileTarget",
            "WorkloadSecretRegistryCredentialTarget",
            "WorkloadSecretBinding",
            "WorkloadProfileResources",
            "WorkloadServicePort",
            "WorkloadHttpHealthCheck",
            "ScheduledTaskRetryPolicy",
            "ScheduledTaskHistoryPolicy",
            "ScheduledTaskSchedule",
            "WorkloadProfileSpec",
            "AcceptedWorkloadProfileRevision",
            "WorkloadProfileMutation",
        ] {
            assert_eq!(
                schemas[name]["additionalProperties"].as_bool(),
                Some(false),
                "{name} must remain closed"
            );
        }
        let accepted = &schemas["AcceptedWorkloadProfileRevision"]["properties"];
        assert_eq!(
            accepted["contractAcl"]["maxLength"].as_u64(),
            Some(WORKLOAD_PROFILE_MAX_ACL_BYTES as u64)
        );
        assert_eq!(
            accepted["contractAcl"]["example"].as_str(),
            Some(WORKLOAD_PROFILE_ACL_EXAMPLE)
        );
        assert!(accepted.get("secretValue").is_none());
        assert_eq!(
            schemas["AcceptedWorkloadProfileRevisionList"]["maxItems"].as_u64(),
            Some(MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT as u64)
        );
        assert_eq!(
            schemas["WorkloadProcess"]["properties"]["command"]["maxItems"].as_u64(),
            Some(MAX_WORKLOAD_PROCESS_COMMANDS as u64)
        );
        assert_eq!(
            schemas["WorkloadProcess"]["properties"]["environment"]["maxProperties"].as_u64(),
            Some(MAX_WORKLOAD_ENVIRONMENT_VARIABLES as u64)
        );
        assert_eq!(
            schemas["WorkloadProfileResources"]["properties"]["cpuMillis"]["maximum"].as_u64(),
            Some(MAX_WORKLOAD_RESOURCE_CPU_MILLIS)
        );
        assert_eq!(
            schemas["AcceptedWorkloadProfileRevision"]["properties"]["revisionNumber"]["maximum"]
                .as_u64(),
            Some(MAX_WORKLOAD_PROFILE_SAFE_INTEGER)
        );
        assert_eq!(
            schemas["WorkloadSecretBinding"]["properties"]["version"]["maximum"].as_u64(),
            Some(MAX_WORKLOAD_PROFILE_SAFE_INTEGER)
        );
        for property in ["intervalMs", "timeoutMs", "stabilizationWindowMs"] {
            assert_eq!(
                schemas["WorkloadHttpHealthCheck"]["properties"][property]["maximum"].as_u64(),
                Some(MAX_WORKLOAD_PROFILE_SAFE_INTEGER),
                "{property} must preserve portable JSON integer semantics"
            );
        }
        assert_eq!(
            schemas["WorkloadProfileSpec"]["properties"]["secrets"]["maxItems"].as_u64(),
            Some(MAX_WORKLOAD_SECRET_BINDINGS as u64)
        );
        assert_eq!(
            schemas["ScheduledTaskSchedule"]["properties"]["maximumConcurrency"]["maximum"]
                .as_u64(),
            Some(u64::from(MAX_WORKLOAD_SCHEDULE_CONCURRENCY))
        );

        let request = accept_workload_profile_request_schema();
        assert_eq!(request["required"], json!(["buildPlanId", "profileAcl"]));
        assert_eq!(request["additionalProperties"], false);
        assert!(request["properties"].get("profile").is_none());
    }
}

use serde_json::{json, Value};

pub(super) fn closed_json_request_schema(path: &str) -> Option<Value> {
    let schema = match path {
        "/bootstrap" => bootstrap_schema(),
        "/node-control/enroll" => node_enrollment_schema(),
        "/organizations" => named_resource_schema(),
        "/webhooks/github" => github_webhook_schema(),
        "/organizations/{organization_id}/api-tokens" => api_token_schema(),
        "/organizations/{organization_id}/assets" => asset_schema(),
        "/organizations/{organization_id}/assets/{asset_id}/releases" => {
            asset_release_schema()
        }
        "/organizations/{organization_id}/domain-claims/{claim_id}/revoke" => {
            reason_schema()
        }
        "/organizations/{organization_id}/domain-claims/{claim_id}/verify" => proof_schema(),
        "/organizations/{organization_id}/enrollment-tokens" => enrollment_token_schema(),
        "/organizations/{organization_id}/memberships" => membership_schema(),
        "/organizations/{organization_id}/memberships/{membership_id}/revocation" => {
            expected_version_schema("expectedVersion")
        }
        "/organizations/{organization_id}/memberships/{membership_id}/role" => {
            membership_role_schema()
        }
        "/organizations/{organization_id}/recipient-contacts" => {
            recipient_contact_verification_request_schema()
        }
        "/organizations/{organization_id}/recipient-contacts/{recipient_contact_id}/verification" => {
            recipient_contact_verification_completion_schema()
        }
        "/organizations/{organization_id}/recipient-contacts/{recipient_contact_id}/revocation" => {
            expected_version_schema("expectedVersion")
        }
        "/organizations/{organization_id}/mcp-credentials/{credential_id}/revoke" => {
            expected_version_schema("expectedAggregateVersion")
        }
        "/organizations/{organization_id}/mcp-credentials/{credential_id}/rotate" => {
            rotate_mcp_credential_schema()
        }
        "/organizations/{organization_id}/projects" => named_resource_schema(),
        "/organizations/{organization_id}/projects/{project_id}/environments" => {
            named_resource_schema()
        }
        "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/domain-claims" => {
            domain_claim_schema()
        }
        "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/executions" => {
            execution_schema()
        }
        "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/gateway-scopes" => {
            gateway_scope_schema()
        }
        "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials" => {
            create_mcp_credential_schema()
        }
        "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/routes" => {
            route_schema()
        }
        "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/secrets" => {
            create_secret_schema()
        }
        "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/source-revisions" => {
            source_revision_schema()
        }
        "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/source-subscriptions/github" => {
            github_subscription_schema()
        }
        "/organizations/{organization_id}/projects/{project_id}/execution-templates" => {
            execution_template_schema()
        }
        "/organizations/{organization_id}/secrets/{secret_id}/versions" => {
            secret_value_schema()
        }
        "/organizations/{organization_id}/workloads/{workload_id}/rollback" => {
            rollback_schema()
        }
        _ if path.ends_with("/agent-conversations/{conversation_id}/executions") => {
            agent_execution_schema()
        }
        _ if path.ends_with("/nodes/{node_id}/actions/drain")
            || path.ends_with("/nodes/{node_id}/actions/ready")
            || path.ends_with("/nodes/{node_id}/actions/revoke") =>
        {
            expected_version_schema("expectedVersion")
        }
        _ if path.ends_with("/assets/{asset_id}/releases/{asset_release_id}/workloads")
            || path.ends_with("/source-revisions/{source_revision_id}/workloads") =>
        {
            create_workload_schema(false)
        }
        _ if path.ends_with("/environments/{environment_id}/workloads") => {
            create_workload_schema(true)
        }
        _ if path.ends_with("/workloads/{workload_id}/assets/{asset_id}/releases/{asset_release_id}/deployments") => {
            update_workload_schema(false)
        }
        _ if path.ends_with("/workloads/{workload_id}/deployments") => {
            update_workload_schema(true)
        }
        _ => return None,
    };
    let mut schema = schema;
    schema["x-a3s-contract-correction"] = json!("documents-existing-runtime-validation");
    Some(schema)
}

fn object(required: &[&str], properties: Value) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties
    })
}

fn named_resource_schema() -> Value {
    object(
        &["name"],
        json!({
            "name": { "type": "string", "minLength": 1, "maxLength": 63 }
        }),
    )
}

fn bootstrap_schema() -> Value {
    object(
        &["organizationName", "tokenName", "token"],
        json!({
            "organizationName": { "type": "string", "minLength": 1, "maxLength": 63 },
            "tokenName": { "type": "string", "minLength": 1, "maxLength": 63 },
            "token": {
                "type": "string",
                "writeOnly": true,
                "pattern": "^a3s_[0-9a-f]{64}$"
            },
            "expiresAt": { "type": "string", "format": "date-time", "nullable": true }
        }),
    )
}

fn node_enrollment_schema() -> Value {
    object(
        &[
            "schema",
            "enrollment_token",
            "node_name",
            "agent_instance_id",
            "agent_version",
            "csr_pem",
            "runtime_capabilities",
        ],
        json!({
            "schema": {
                "type": "string",
                "enum": ["a3s.cloud.node-enrollment-request.v1"]
            },
            "enrollment_token": {
                "type": "string",
                "writeOnly": true,
                "pattern": "^a3sn_[0-9a-f]{64}$"
            },
            "node_name": { "type": "string", "minLength": 1, "maxLength": 255 },
            "agent_instance_id": uuid_schema(),
            "agent_version": { "type": "string", "minLength": 1, "maxLength": 255 },
            "csr_pem": {
                "type": "string",
                "writeOnly": true,
                "maxLength": 131072,
                "pattern": "^-----BEGIN CERTIFICATE REQUEST-----"
            },
            "runtime_capabilities": runtime_capabilities_schema()
        }),
    )
}

fn runtime_capabilities_schema() -> Value {
    object(
        &[
            "schema",
            "provider_id",
            "provider_build",
            "unit_classes",
            "artifact_media_types",
            "isolation_levels",
            "network_modes",
            "mount_kinds",
            "health_check_kinds",
            "resource_controls",
            "features",
        ],
        json!({
            "schema": { "type": "string", "enum": ["a3s.runtime.capabilities.v4"] },
            "provider_id": { "type": "string", "minLength": 1, "maxLength": 255 },
            "provider_build": { "type": "string", "minLength": 1, "maxLength": 255 },
            "unit_classes": string_enum_array(&["task", "service"], true),
            "artifact_media_types": {
                "type": "array", "minItems": 1, "uniqueItems": true,
                "items": { "type": "string", "minLength": 1, "maxLength": 255 }
            },
            "isolation_levels": string_enum_array(&["process", "container", "sandbox", "confidential"], true),
            "network_modes": string_enum_array(&["none", "outbound", "service"], false),
            "mount_kinds": string_enum_array(&["artifact", "volume", "tmpfs"], false),
            "health_check_kinds": string_enum_array(&["http", "tcp", "command"], false),
            "resource_controls": string_enum_array(&["cpu", "memory", "pids", "ephemeral_storage", "execution_timeout"], true),
            "features": string_enum_array(&[
                "durable_identity", "stop", "remove", "service_tcp", "service_udp",
                "logs", "exec", "usage", "attestation", "secret_references", "output_artifacts"
            ], false)
        }),
    )
}

fn api_token_schema() -> Value {
    object(
        &["name", "token", "scopes"],
        json!({
            "name": { "type": "string", "minLength": 1, "maxLength": 63 },
            "token": {
                "type": "string",
                "writeOnly": true,
                "pattern": "^a3s_[0-9a-f]{64}$"
            },
            "scopes": {
                "type": "array", "minItems": 1, "uniqueItems": true,
                "items": {
                    "type": "string", "maxLength": 63,
                    "pattern": "^[a-z-]+:[a-z-]+$"
                }
            },
            "principalId": { "type": "string", "format": "uuid", "nullable": true },
            "expiresAt": { "type": "string", "format": "date-time", "nullable": true }
        }),
    )
}

fn membership_schema() -> Value {
    object(
        &["name", "role"],
        json!({
            "principalKind": {
                "type": "string", "enum": ["human", "service"], "default": "service"
            },
            "name": { "type": "string", "minLength": 1, "maxLength": 63 },
            "role": { "type": "string", "enum": ["owner", "admin", "member", "restricted"] }
        }),
    )
}

fn membership_role_schema() -> Value {
    object(
        &["role", "expectedVersion"],
        json!({
            "role": { "type": "string", "enum": ["owner", "admin", "member", "restricted"] },
            "expectedVersion": positive_integer_schema()
        }),
    )
}

fn recipient_contact_verification_request_schema() -> Value {
    object(
        &["address"],
        json!({
            "address": {
                "type": "string",
                "format": "email",
                "minLength": 3,
                "maxLength": 254,
                "writeOnly": true
            }
        }),
    )
}

fn recipient_contact_verification_completion_schema() -> Value {
    object(
        &["proof"],
        json!({
            "proof": {
                "type": "string",
                "minLength": 1,
                "maxLength": 4096,
                "pattern": "^a3srcv1\\.[A-Za-z0-9_-]+\\.[A-Za-z0-9_-]+$",
                "writeOnly": true
            }
        }),
    )
}

fn asset_schema() -> Value {
    object(
        &["name", "kind"],
        json!({
            "name": { "type": "string", "minLength": 1, "maxLength": 63 },
            "kind": { "type": "string", "enum": ["agent", "mcp", "skill"] }
        }),
    )
}

fn asset_release_schema() -> Value {
    object(
        &["version", "commitSha"],
        json!({
            "version": {
                "type": "string", "minLength": 1, "maxLength": 128,
                "pattern": "^[0-9]+\\.[0-9]+\\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$"
            },
            "commitSha": { "type": "string", "pattern": "^[0-9a-f]{40}$" }
        }),
    )
}

fn agent_execution_schema() -> Value {
    object(
        &["agentAssetId", "agentAssetReleaseId"],
        json!({
            "agentAssetId": uuid_schema(),
            "agentAssetReleaseId": uuid_schema(),
            "providerKind": {
                "type": "string",
                "enum": ["a3s.code", "reference.echo"],
                "default": "a3s.code",
                "description": "Closed provider kind. Cloud resolves and persists the exact immutable provider profile before scheduling."
            },
            "input": {
                "nullable": true,
                "description": "Canonical JSON input passed to the selected Agent release."
            }
        }),
    )
}

fn enrollment_token_schema() -> Value {
    object(
        &["name", "token", "expiresAt"],
        json!({
            "name": { "type": "string", "minLength": 1, "maxLength": 63 },
            "token": {
                "type": "string", "writeOnly": true,
                "pattern": "^a3sn_[0-9a-f]{64}$"
            },
            "expiresAt": { "type": "string", "format": "date-time" }
        }),
    )
}

fn reason_schema() -> Value {
    object(
        &["reason"],
        json!({
            "reason": { "type": "string", "minLength": 1, "maxLength": 4096 }
        }),
    )
}

fn proof_schema() -> Value {
    object(
        &["proof"],
        json!({
            "proof": { "type": "string", "minLength": 1, "maxLength": 4096, "writeOnly": true }
        }),
    )
}

fn expected_version_schema(field: &str) -> Value {
    let mut schema = object(&[field], json!({}));
    schema["properties"][field] = positive_integer_schema();
    schema
}

fn create_mcp_credential_schema() -> Value {
    object(
        &["expiresAt"],
        json!({ "expiresAt": { "type": "string", "format": "date-time" } }),
    )
}

fn rotate_mcp_credential_schema() -> Value {
    object(
        &["expiresAt", "expectedAggregateVersion"],
        json!({
            "expiresAt": { "type": "string", "format": "date-time" },
            "expectedAggregateVersion": positive_integer_schema()
        }),
    )
}

fn domain_claim_schema() -> Value {
    object(
        &["pattern"],
        json!({
            "pattern": { "type": "string", "minLength": 1, "maxLength": 253 }
        }),
    )
}

fn gateway_scope_schema() -> Value {
    let mut schema = object(
        &[],
        json!({
            "nodeId": { "type": "string", "format": "uuid" },
            "nodeIds": {
                "type": "array", "minItems": 1, "uniqueItems": true,
                "items": uuid_schema()
            },
            "minReady": { "type": "integer", "minimum": 1, "default": 1 },
            "maxUnavailable": { "type": "integer", "minimum": 0, "default": 0 }
        }),
    );
    schema["oneOf"] = json!([
        { "required": ["nodeId"], "not": { "required": ["nodeIds"] } },
        { "required": ["nodeIds"], "not": { "required": ["nodeId"] } }
    ]);
    schema
}

fn route_schema() -> Value {
    object(
        &[
            "gatewayScopeId",
            "workloadRevisionId",
            "domainClaimId",
            "hostname",
            "pathPrefix",
            "portName",
        ],
        json!({
            "gatewayScopeId": uuid_schema(),
            "workloadRevisionId": uuid_schema(),
            "domainClaimId": uuid_schema(),
            "hostname": { "type": "string", "minLength": 1, "maxLength": 253 },
            "pathPrefix": { "type": "string", "minLength": 1, "maxLength": 2048, "pattern": "^/" },
            "portName": { "type": "string", "minLength": 1, "maxLength": 63 }
        }),
    )
}

fn create_secret_schema() -> Value {
    object(
        &["name", "value"],
        json!({
            "name": { "type": "string", "minLength": 1, "maxLength": 63 },
            "value": { "type": "string", "minLength": 1, "maxLength": 1048576, "writeOnly": true }
        }),
    )
}

fn secret_value_schema() -> Value {
    object(
        &["value"],
        json!({
            "value": { "type": "string", "minLength": 1, "maxLength": 1048576, "writeOnly": true }
        }),
    )
}

fn execution_template_schema() -> Value {
    object(
        &["definitionAcl"],
        json!({
            "definitionAcl": { "type": "string", "minLength": 1, "maxLength": 131072 }
        }),
    )
}

fn execution_schema() -> Value {
    object(
        &["artifact", "process", "resources"],
        json!({
            "artifact": object(
                &["uri", "digest", "mediaType"],
                json!({
                    "uri": { "type": "string", "minLength": 1, "maxLength": 2048 },
                    "digest": digest_schema(),
                    "mediaType": { "type": "string", "minLength": 1, "maxLength": 255 }
                })
            ),
            "process": process_schema(),
            "input": { "nullable": true, "description": "Canonical JSON execution input." },
            "resources": object(
                &["cpuMillis", "memoryBytes", "pids", "timeoutMs"],
                json!({
                    "cpuMillis": positive_integer_schema(),
                    "memoryBytes": positive_integer_schema(),
                    "pids": positive_integer_schema(),
                    "ephemeralStorageBytes": nullable_positive_integer_schema(),
                    "timeoutMs": positive_integer_schema()
                })
            )
        }),
    )
}

fn create_workload_schema(include_artifact: bool) -> Value {
    object(
        &["name", "template"],
        json!({
            "name": { "type": "string", "minLength": 1, "maxLength": 63 },
            "nodePoolId": { "type": "string", "format": "uuid", "nullable": true },
            "template": service_template_schema(include_artifact)
        }),
    )
}

fn update_workload_schema(include_artifact: bool) -> Value {
    object(
        &["template"],
        json!({ "template": service_template_schema(include_artifact) }),
    )
}

fn service_template_schema(include_artifact: bool) -> Value {
    let mut required = vec!["resources"];
    let mut properties = json!({
        "process": process_schema(),
        "secrets": {
            "type": "array", "default": [],
            "items": secret_binding_schema()
        },
        "resources": object(
            &["cpuMillis", "memoryBytes", "pids"],
            json!({
                "cpuMillis": positive_integer_schema(),
                "memoryBytes": positive_integer_schema(),
                "pids": positive_integer_schema(),
                "ephemeralStorageBytes": nullable_positive_integer_schema()
            })
        ),
        "ports": {
            "type": "array", "default": [],
            "items": object(
                &["name", "containerPort"],
                json!({
                    "name": { "type": "string", "minLength": 1, "maxLength": 63 },
                    "containerPort": { "type": "integer", "minimum": 1, "maximum": 65535 }
                })
            )
        },
        "health": nullable_object(
                &[
                    "portName", "path", "intervalMs", "timeoutMs", "healthyThreshold",
                    "unhealthyThreshold", "stabilizationWindowMs"
                ],
                json!({
                    "portName": { "type": "string", "minLength": 1, "maxLength": 63 },
                    "path": { "type": "string", "minLength": 1, "maxLength": 2048, "pattern": "^/" },
                    "intervalMs": positive_integer_schema(),
                    "timeoutMs": positive_integer_schema(),
                    "healthyThreshold": positive_integer_schema(),
                    "unhealthyThreshold": positive_integer_schema(),
                    "stabilizationWindowMs": { "type": "integer", "minimum": 0 }
                })
            )
    });
    if include_artifact {
        required.insert(0, "artifact");
        properties["artifact"] = object(
            &["uri"],
            json!({
                "uri": { "type": "string", "minLength": 1, "maxLength": 2048 },
                "expectedDigest": {
                    "type": "string", "pattern": "^sha256:[0-9a-f]{64}$", "nullable": true
                }
            }),
        );
    }
    object(&required, properties)
}

fn process_schema() -> Value {
    object(
        &[],
        json!({
            "command": {
                "type": "array", "default": [],
                "items": { "type": "string", "minLength": 1, "maxLength": 4096 }
            },
            "args": {
                "type": "array", "default": [],
                "items": { "type": "string", "maxLength": 4096 }
            },
            "workingDirectory": { "type": "string", "minLength": 1, "maxLength": 4096, "nullable": true },
            "environment": {
                "type": "object", "default": {},
                "additionalProperties": { "type": "string", "maxLength": 65536 }
            }
        }),
    )
}

fn secret_binding_schema() -> Value {
    object(
        &["name", "secretId", "version", "target"],
        json!({
            "name": { "type": "string", "minLength": 1, "maxLength": 63 },
            "secretId": uuid_schema(),
            "version": positive_integer_schema(),
            "target": {
                "oneOf": [
                    object(
                        &["kind", "variable"],
                        json!({
                            "kind": { "type": "string", "enum": ["environment"] },
                            "variable": { "type": "string", "minLength": 1, "maxLength": 255 }
                        })
                    ),
                    object(
                        &["kind", "path", "mode"],
                        json!({
                            "kind": { "type": "string", "enum": ["file"] },
                            "path": { "type": "string", "minLength": 1, "maxLength": 4096 },
                            "mode": { "type": "integer", "minimum": 0, "maximum": 511 }
                        })
                    ),
                    object(
                        &["kind"],
                        json!({ "kind": { "type": "string", "enum": ["registry_credential"] } })
                    )
                ],
                "discriminator": { "propertyName": "kind" }
            }
        }),
    )
}

fn source_revision_schema() -> Value {
    object(
        &["repository", "reference", "recipe"],
        json!({
            "repository": git_repository_schema(),
            "reference": object(
                &["kind", "value"],
                json!({
                    "kind": { "type": "string", "enum": ["branch", "tag", "commit"] },
                    "value": { "type": "string", "minLength": 1, "maxLength": 255 }
                })
            ),
            "recipe": build_recipe_schema(),
            "webhookDeliveryId": { "type": "string", "minLength": 1, "maxLength": 255 }
        }),
    )
}

fn github_subscription_schema() -> Value {
    object(
        &["repository", "branch", "recipe"],
        json!({
            "repository": git_repository_schema(),
            "branch": { "type": "string", "minLength": 1, "maxLength": 255 },
            "recipe": build_recipe_schema()
        }),
    )
}

fn git_repository_schema() -> Value {
    object(
        &["provider", "url"],
        json!({
            "provider": { "type": "string", "enum": ["github"] },
            "url": { "type": "string", "format": "uri", "maxLength": 2048 }
        }),
    )
}

fn build_recipe_schema() -> Value {
    object(
        &[
            "schema",
            "kind",
            "contextPath",
            "dockerfilePath",
            "platforms",
        ],
        json!({
            "schema": { "type": "string", "enum": ["a3s.cloud.build-recipe.v1"] },
            "kind": { "type": "string", "enum": ["dockerfile"] },
            "contextPath": { "type": "string", "minLength": 1, "maxLength": 4096 },
            "dockerfilePath": { "type": "string", "minLength": 1, "maxLength": 4096 },
            "target": { "type": "string", "minLength": 1, "maxLength": 255, "nullable": true },
            "platforms": {
                "type": "array", "minItems": 1, "uniqueItems": true,
                "items": { "type": "string", "enum": ["linux/amd64", "linux/arm64"] }
            }
        }),
    )
}

fn rollback_schema() -> Value {
    object(&["revisionId"], json!({ "revisionId": uuid_schema() }))
}

fn github_webhook_schema() -> Value {
    json!({
        "oneOf": [
            object(
                &["ref", "after", "deleted", "repository", "installation"],
                json!({
                    "ref": { "type": "string", "minLength": 1, "maxLength": 1024 },
                    "after": { "type": "string", "pattern": "^[0-9a-f]{40}$" },
                    "deleted": { "type": "boolean" },
                    "repository": object(
                        &["full_name", "html_url"],
                        json!({
                            "full_name": { "type": "string", "minLength": 3, "maxLength": 255 },
                            "html_url": { "type": "string", "format": "uri" }
                        })
                    ),
                    "installation": object(
                        &["id"],
                        json!({ "id": positive_integer_schema() })
                    )
                })
            ),
            {
                "type": "object",
                "additionalProperties": true,
                "required": ["action", "sender"],
                "properties": {
                    "action": { "type": "string", "minLength": 1, "maxLength": 255 },
                    "sender": object(
                        &["id", "login"],
                        json!({
                            "id": positive_integer_schema(),
                            "login": { "type": "string", "minLength": 1, "maxLength": 255 }
                        })
                    )
                }
            }
        ],
        "description": "GitHub-owned push or App installation lifecycle payload. The x-github-event header selects the exact payload variant."
    })
}

fn uuid_schema() -> Value {
    json!({ "type": "string", "format": "uuid" })
}

fn digest_schema() -> Value {
    json!({ "type": "string", "pattern": "^sha256:[0-9a-f]{64}$" })
}

fn positive_integer_schema() -> Value {
    json!({ "type": "integer", "minimum": 1 })
}

fn nullable_positive_integer_schema() -> Value {
    json!({ "type": "integer", "minimum": 1, "nullable": true })
}

fn nullable_object(required: &[&str], properties: Value) -> Value {
    let mut schema = object(required, properties);
    schema["nullable"] = json!(true);
    schema
}

fn string_enum_array(values: &[&str], required: bool) -> Value {
    let mut schema = json!({
        "type": "array",
        "uniqueItems": true,
        "items": { "type": "string", "enum": values }
    });
    if required {
        schema["minItems"] = json!(1);
    }
    schema
}

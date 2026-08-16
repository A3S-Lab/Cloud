use super::*;
use crate::modules::artifacts::BuildRun;
use crate::modules::data::ObjectNamespaceRetentionPolicySpec;
use crate::modules::durable_cells::domain::{
    DurableCellApplicationDefinition, DurableCellApplicationDefinitionSpec, DurableCellClassSpec,
    DurableCellDeploymentBinding, DurableCellDeploymentBindingSpec, DurableCellRollbackPolicy,
    DurableCellServiceProfile, DurableCellServiceProfileSpec, DurableCellStateSchema,
};
use crate::modules::secrets::domain::{CreateSecretWrite, Secret, SecretChanged};
use crate::modules::shared_kernel::domain::{
    BuildRunId, ResourceName, SecretId, SecretVersionReference, Sha256Digest, SourceRevisionId,
};

const CELL_WRITE_TOKEN: &str =
    "a3s_c511111111111111111111111111111111111111111111111111111111111111";
const CELL_READ_TOKEN: &str =
    "a3s_c522222222222222222222222222222222222222222222222222222222222222";

#[tokio::test]
async fn durable_cell_rest_surface_reuses_c2_and_acl_native_c3() -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let secrets = Arc::new(InMemorySecretRepository::new());
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let builds = Arc::new(InMemoryBuildRunRepository::new());
    let app = build_test_application_with_external_builds(
        identity,
        projects,
        Arc::clone(&secrets),
        workloads,
        Arc::new(InMemorySourceRevisionRepository::new()),
        Arc::clone(&builds),
    )?;

    let organization = bootstrap_organization(&app, "cells-bootstrap", "Durable Cells").await?;
    let project = create_project(&app, &organization, "cells-project", "Cells project").await?;
    let environment = create_cell_environment(
        &app,
        &organization,
        &project,
        "cells-environment",
        "Production",
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "cells-writer-token",
        "cells-writer",
        CELL_WRITE_TOKEN,
        &[ApiTokenScope::WORKLOAD_WRITE],
        None,
    )
    .await?;
    create_api_token(
        &app,
        &organization,
        "cells-reader-token",
        "cells-reader",
        CELL_READ_TOKEN,
        &[ApiTokenScope::CLOUD_READ],
        None,
    )
    .await?;

    let organization_id =
        OrganizationId::from_uuid(parse_cell_uuid(&organization, "organization")?);
    let project_id = ProjectId::from_uuid(parse_cell_uuid(&project, "project")?);
    let environment_id = EnvironmentId::from_uuid(parse_cell_uuid(&environment, "environment")?);
    let build = BuildRun::reserve(
        organization_id,
        project_id,
        environment_id,
        SourceRevisionId::new(),
        Utc::now(),
    );
    let build_run_id = build.id;
    builds.seed_build(build).await;
    let access_key = store_cell_secret(
        secrets.as_ref(),
        organization_id,
        project_id,
        environment_id,
        "S0 access key",
    )
    .await?;
    let secret_key = store_cell_secret(
        secrets.as_ref(),
        organization_id,
        project_id,
        environment_id,
        "S0 secret key",
    )
    .await?;

    let profile = service_profile()?;
    let applications_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/durable-cell-applications"
    );
    let definition = application_definition(build_run_id, &profile, 'a')?;
    let create_body = json!({
        "name": "Tenant counters",
        "definitionAcl": definition.canonical_acl(),
    });
    assert_eq!(
        app.call(post_json_as(
            &applications_path,
            "cells-create-read-denied",
            create_body.clone(),
            CELL_READ_TOKEN,
        ))
        .await?
        .status(),
        403
    );
    let created = app
        .call(post_json_as(
            &applications_path,
            "cells-create",
            create_body.clone(),
            CELL_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(created.status(), 201);
    let created = response_json(&created)?;
    assert_eq!(created["data"]["replayed"], false);
    assert_eq!(
        created["data"]["record"]["revision"]["definitionAcl"],
        definition.canonical_acl()
    );
    let application_id = required_cell_string(
        &created["data"]["record"]["application"]["applicationId"],
        "application ID",
    )?;
    let initial_revision_id = required_cell_string(
        &created["data"]["record"]["revision"]["revisionId"],
        "initial revision ID",
    )?;

    let replay = app
        .call(post_json_as(
            &applications_path,
            "cells-create",
            create_body,
            CELL_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(replay.status(), 200);
    assert_eq!(response_json(&replay)?["data"]["replayed"], true);
    assert_eq!(
        app.call(get_as(&applications_path, CELL_WRITE_TOKEN))
            .await?
            .status(),
        403
    );
    let listed = app
        .call(get_as(&applications_path, CELL_READ_TOKEN))
        .await?;
    assert_eq!(listed.status(), 200);
    assert_eq!(
        response_json(&listed)?["data"][0]["applicationId"],
        application_id
    );
    assert_eq!(
        app.call(get_as(
            format!("{applications_path}?limit=201"),
            CELL_READ_TOKEN,
        ))
        .await?
        .status(),
        400
    );

    let application_path = format!("{applications_path}/{application_id}");
    let stop = app
        .call(post_json_as(
            format!("{application_path}/stop"),
            "cells-stop",
            json!({"expectedVersion": 1}),
            CELL_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(stop.status(), 200);
    assert_eq!(
        response_json(&stop)?["data"]["record"]["application"]["desiredState"],
        "stopped"
    );
    let start = app
        .call(post_json_as(
            format!("{application_path}/start"),
            "cells-start",
            json!({"expectedVersion": 2}),
            CELL_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(start.status(), 200);
    assert_eq!(
        response_json(&start)?["data"]["record"]["application"]["aggregateVersion"],
        3
    );

    let revisions_path = format!("{application_path}/revisions");
    let revised_definition = application_definition(build_run_id, &profile, 'b')?;
    let revised = app
        .call(post_json_as(
            &revisions_path,
            "cells-revise",
            json!({
                "expectedVersion": 3,
                "definitionAcl": revised_definition.canonical_acl(),
            }),
            CELL_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(revised.status(), 201);
    let revised = response_json(&revised)?;
    assert_eq!(
        revised["data"]["record"]["revision"]["parentRevisionId"],
        initial_revision_id
    );
    let revision_id = required_cell_string(
        &revised["data"]["record"]["revision"]["revisionId"],
        "revision ID",
    )?;
    let history = app.call(get_as(&revisions_path, CELL_READ_TOKEN)).await?;
    assert_eq!(history.status(), 200);
    assert_eq!(
        response_json(&history)?["data"].as_array().map(Vec::len),
        Some(2)
    );

    let deployment_path = format!("{revisions_path}/{revision_id}/deployments");
    let storage = deployment_binding(access_key, secret_key)?;
    let provider_acl = provider_workload_acl(&profile, access_key, secret_key, true);
    let deployment_body = json!({
        "serviceProfileAcl": profile.canonical_acl(),
        "providerWorkloadAcl": provider_acl,
        "storageBindingAcl": storage.canonical_acl(),
    });
    assert_eq!(
        app.call(post_json_as(
            &deployment_path,
            "cells-deploy-read-denied",
            deployment_body.clone(),
            CELL_READ_TOKEN,
        ))
        .await?
        .status(),
        403
    );
    let deployed = app
        .call(post_json_as(
            &deployment_path,
            "cells-deploy",
            deployment_body.clone(),
            CELL_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(deployed.status(), 201);
    let deployed = response_json(&deployed)?;
    assert_eq!(deployed["data"]["replayed"], false);
    assert_eq!(
        deployed["data"]["correlation"]["applicationRevisionId"],
        revision_id
    );
    assert_eq!(
        deployed["data"]["correlation"]["serviceProfileDigest"],
        profile.digest().as_str()
    );
    assert_eq!(deployed["data"]["workload"]["replayed"], false);
    assert!(deployed["data"]["correlation"]
        .get("secretAccessKey")
        .is_none());

    let replayed = app
        .call(post_json_as(
            &deployment_path,
            "cells-deploy",
            deployment_body,
            CELL_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(replayed.status(), 200);
    assert_eq!(response_json(&replayed)?["data"]["replayed"], true);

    let unpinned = app
        .call(post_json_as(
            &deployment_path,
            "cells-deploy-unpinned",
            json!({
                "serviceProfileAcl": profile.canonical_acl(),
                "providerWorkloadAcl": provider_workload_acl(
                    &profile,
                    access_key,
                    secret_key,
                    false,
                ),
                "storageBindingAcl": storage.canonical_acl(),
            }),
            CELL_WRITE_TOKEN,
        ))
        .await?;
    assert_eq!(unpinned.status(), 422);
    Ok(())
}

pub(super) fn application_definition(
    build_run_id: BuildRunId,
    profile: &DurableCellServiceProfile,
    marker: char,
) -> Result<DurableCellApplicationDefinition> {
    DurableCellApplicationDefinition::from_spec(DurableCellApplicationDefinitionSpec {
        build_run_id,
        bundle_digest: digest(marker)?,
        bundle_size_bytes: 4096,
        main_module: "worker.mjs".into(),
        compatibility_date: "2026-08-16".into(),
        compatibility_flags: Vec::new(),
        cell_classes: vec![DurableCellClassSpec {
            name: "Counter".into(),
            state_schema: DurableCellStateSchema {
                minimum_readable_version: 1,
                maximum_readable_version: 1,
                write_version: 1,
            },
        }],
        service_profile_digest: profile.digest().clone(),
        rollback_policy: DurableCellRollbackPolicy::Compatible,
    })
    .map_err(BootError::Internal)
}

pub(super) fn service_profile() -> Result<DurableCellServiceProfile> {
    DurableCellServiceProfile::from_spec(DurableCellServiceProfileSpec {
        public_runtime_port: "cell-public".into(),
        internal_runtime_port: "cell-internal".into(),
        health_path: "/__celld/health".into(),
        max_cell_name_bytes: 512,
        max_request_bytes: 16 * 1024 * 1024,
        max_response_bytes: 64 * 1024 * 1024,
        max_websocket_message_bytes: 1024 * 1024,
    })
    .map_err(BootError::Internal)
}

pub(super) fn deployment_binding(
    access_key: SecretVersionReference,
    secret_key: SecretVersionReference,
) -> Result<DurableCellDeploymentBinding> {
    DurableCellDeploymentBinding::from_spec(DurableCellDeploymentBindingSpec {
        credential_generation: 1,
        provider_profile_digest: digest('c')?,
        access_key_id: access_key,
        secret_access_key: secret_key,
        session_token: None,
        retention_policy: ObjectNamespaceRetentionPolicySpec {
            minimum_sealed_recovery_points: 2,
            maximum_sealed_recovery_points: 24,
            maximum_recovery_point_age_seconds: 30 * 24 * 60 * 60,
            deletion_grace_period_seconds: 24 * 60 * 60,
        },
    })
    .map_err(BootError::Internal)
}

pub(super) fn provider_workload_acl(
    profile: &DurableCellServiceProfile,
    access_key: SecretVersionReference,
    secret_key: SecretVersionReference,
    pinned: bool,
) -> String {
    let artifact_digest = format!("sha256:{}", "d".repeat(64));
    let artifact_uri = if pinned {
        format!("oci://ghcr.io/denoland/celld@{artifact_digest}")
    } else {
        "oci://ghcr.io/denoland/celld:latest".into()
    };
    let expected_digest = if pinned {
        format!("    expected_digest = \"{artifact_digest}\"\n")
    } else {
        String::new()
    };
    format!(
        r#"version = 1

workload "celld-provider" {{
  artifact {{
    uri = "{artifact_uri}"
{expected_digest}  }}
  process {{
    command = ["/usr/local/bin/celld"]
    args = ["--listen", "0.0.0.0:8080", "--internal-listen", "0.0.0.0:8081"]
    working_directory = "/"
  }}
  resources {{
    cpu_millis = 1000
    memory_bytes = 536870912
    pids = 256
    ephemeral_storage_bytes = 536870912
  }}
  port "{}" {{
    container_port = 8080
  }}
  port "{}" {{
    container_port = 8081
  }}
  health {{
    port_name = "{}"
    path = "{}"
    interval_ms = 1000
    timeout_ms = 500
    healthy_threshold = 1
    unhealthy_threshold = 3
    stabilization_window_ms = 5000
  }}
  secret "s0-access-key-id" {{
    secret_id = "{}"
    version = {}
    environment {{
      variable = "S0_ACCESS_KEY_ID"
    }}
  }}
  secret "s0-secret-access-key" {{
    secret_id = "{}"
    version = {}
    environment {{
      variable = "S0_SECRET_ACCESS_KEY"
    }}
  }}
}}
"#,
        profile.spec().public_runtime_port,
        profile.spec().internal_runtime_port,
        profile.spec().public_runtime_port,
        profile.spec().health_path,
        access_key.secret_id,
        access_key.version,
        secret_key.secret_id,
        secret_key.version,
    )
}

pub(super) async fn store_cell_secret(
    secrets: &InMemorySecretRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    name: &str,
) -> Result<SecretVersionReference> {
    let secret_id = SecretId::new();
    let now = Utc::now();
    let (secret, version) = Secret::create(
        secret_id,
        organization_id,
        project_id,
        environment_id,
        ResourceName::parse(name).map_err(BootError::Internal)?,
        EncryptedSecretValue::new("test-key", format!("ciphertext-{secret_id}"))
            .map_err(BootError::Internal)?,
        now,
    )
    .map_err(BootError::Internal)?;
    secrets
        .create(CreateSecretWrite {
            event: SecretChanged::created(&secret, &version, Uuid::now_v7())
                .map_err(|error| BootError::Internal(error.to_string()))?,
            idempotency: IdempotencyRequest::new(
                "tests/durable-cell/rest/secrets",
                secret_id.to_string(),
                secret_id.as_uuid().as_bytes(),
            )
            .map_err(BootError::Internal)?,
            secret,
            version,
        })
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;
    SecretVersionReference::new(secret_id, 1).map_err(BootError::Internal)
}

fn digest(marker: char) -> Result<Sha256Digest> {
    Sha256Digest::parse(format!("sha256:{}", marker.to_string().repeat(64)))
        .map_err(BootError::Internal)
}

pub(super) fn required_cell_string(value: &Value, label: &str) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| BootError::Internal(format!("Durable Cell response has no {label}")))
}

pub(super) async fn create_cell_environment(
    app: &BootApplication,
    organization: &str,
    project: &str,
    key: &str,
    name: &str,
) -> Result<String> {
    let response = app
        .call(post_json(
            format!("/api/v1/organizations/{organization}/projects/{project}/environments"),
            key,
            json!({"name": name}),
        ))
        .await?;
    if response.status() != 201 {
        return Err(BootError::Internal(format!(
            "failed to create Durable Cell test environment: {}",
            response.status()
        )));
    }
    response_id(&response)
}

pub(super) fn parse_cell_uuid(value: &str, label: &str) -> Result<Uuid> {
    Uuid::parse_str(value)
        .map_err(|error| BootError::Internal(format!("invalid {label} ID: {error}")))
}

use super::*;
use crate::modules::shared_kernel::domain::{DeploymentId, NodeCommandId, NodeId, OrganizationId};

#[tokio::test]
async fn workload_update_api_requires_an_active_revision_and_creates_one_idempotent_generation(
) -> Result<()> {
    let identity = Arc::new(InMemoryIdentityRepository::new());
    let projects = Arc::new(InMemoryProjectsRepository::new());
    let secrets = Arc::new(InMemorySecretRepository::new());
    let workloads = Arc::new(InMemoryWorkloadRepository::new());
    let app = build_test_application_with_repositories(
        identity,
        projects,
        secrets,
        Arc::clone(&workloads),
    )?;
    let organization = bootstrap_organization(&app, "update-bootstrap", "Acme").await?;
    let project = create_project(&app, &organization, "update-project", "Cloud").await?;
    let environment_path =
        format!("/api/v1/organizations/{organization}/projects/{project}/environments");
    let environment = app
        .call(post_json(
            &environment_path,
            "update-environment",
            json!({"name": "Production"}),
        ))
        .await?;
    assert_eq!(environment.status(), 201);
    let environment = response_id(&environment)?;
    let workloads_path = format!(
        "/api/v1/organizations/{organization}/projects/{project}/environments/{environment}/workloads"
    );
    let created = app
        .call(post_json(
            &workloads_path,
            "update-workload",
            json!({
                "name": "api",
                "template": workload_template("v1", json!([]))
            }),
        ))
        .await?;
    assert_eq!(created.status(), 202);
    let created_json = response_json(&created)?;
    let workload_id = created_json["data"]["workloadId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("workload response has no workload ID".into()))?;
    let deployment_id = created_json["data"]["deploymentId"]
        .as_str()
        .ok_or_else(|| BootError::Internal("workload response has no deployment ID".into()))?;
    let update_path =
        format!("/api/v1/organizations/{organization}/workloads/{workload_id}/deployments");
    let inactive = app
        .call(post_json(
            &update_path,
            "update-inactive",
            json!({"template": workload_template("v2", json!([]))}),
        ))
        .await?;
    assert_eq!(inactive.status(), 409);

    let organization_id = OrganizationId::from_uuid(
        Uuid::parse_str(&organization).map_err(|error| BootError::Internal(error.to_string()))?,
    );
    let deployment_id = DeploymentId::from_uuid(
        Uuid::parse_str(deployment_id).map_err(|error| BootError::Internal(error.to_string()))?,
    );
    let mut deployment = workloads
        .find_deployment(organization_id, deployment_id)
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;
    deployment = workloads
        .mark_resolving(
            deployment.id,
            deployment.aggregate_version,
            Utc::now().max(deployment.updated_at),
        )
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;
    deployment = workloads
        .assign_node(
            deployment.id,
            deployment.aggregate_version,
            NodeId::new(),
            Utc::now().max(deployment.updated_at),
        )
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;
    deployment = workloads
        .mark_dispatched(
            deployment.id,
            deployment.aggregate_version,
            NodeCommandId::new(),
            Utc::now().max(deployment.updated_at),
        )
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;
    deployment = workloads
        .mark_verifying(
            deployment.id,
            deployment.aggregate_version,
            Utc::now().max(deployment.updated_at),
        )
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;
    workloads
        .activate(
            deployment.id,
            deployment.aggregate_version,
            false,
            Utc::now().max(deployment.updated_at),
        )
        .await
        .map_err(|error| BootError::Internal(error.to_string()))?;

    let invalid_secret = app
        .call(post_json(
            &update_path,
            "update-invalid-secret",
            json!({
                "template": workload_template(
                    "v2",
                    json!([{
                        "name": "database-url",
                        "secretId": Uuid::now_v7(),
                        "version": 1,
                        "target": {
                            "kind": "environment",
                            "variable": "DATABASE_URL"
                        }
                    }])
                )
            }),
        ))
        .await?;
    assert_eq!(invalid_secret.status(), 422);

    let request = || {
        post_json(
            &update_path,
            "update-v2",
            json!({"template": workload_template("v2", json!([]))}),
        )
    };
    let accepted = app.call(request()).await?;
    let replayed = app.call(request()).await?;
    assert_eq!(accepted.status(), 202);
    assert_eq!(replayed.status(), 200);
    let accepted_json = response_json(&accepted)?;
    let replayed_json = response_json(&replayed)?;
    assert_eq!(accepted_json["data"]["generation"], 2);
    assert_eq!(
        accepted_json["data"]["deploymentId"],
        replayed_json["data"]["deploymentId"]
    );
    assert_eq!(
        accepted_json["data"]["revisionId"],
        replayed_json["data"]["revisionId"]
    );
    assert_eq!(replayed_json["data"]["replayed"], true);

    let changed_replay = app
        .call(post_json(
            &update_path,
            "update-v2",
            json!({"template": workload_template("changed", json!([]))}),
        ))
        .await?;
    assert_eq!(changed_replay.status(), 409);
    let concurrent = app
        .call(post_json(
            &update_path,
            "update-v3-concurrent",
            json!({"template": workload_template("v3", json!([]))}),
        ))
        .await?;
    assert_eq!(concurrent.status(), 409);
    Ok(())
}

fn workload_template(tag: &str, secrets: Value) -> Value {
    json!({
        "artifact": {
            "uri": format!("oci://registry.example/cloud/api:{tag}"),
            "expectedDigest": null
        },
        "process": {},
        "secrets": secrets,
        "resources": {
            "cpuMillis": 100,
            "memoryBytes": 33554432,
            "pids": 32,
            "ephemeralStorageBytes": null
        },
        "ports": [{"name": "http", "containerPort": 8080}],
        "health": {
            "portName": "http",
            "path": "/health",
            "intervalMs": 1000,
            "timeoutMs": 500,
            "healthyThreshold": 1,
            "unhealthyThreshold": 3,
            "stabilizationWindowMs": 1000
        }
    })
}

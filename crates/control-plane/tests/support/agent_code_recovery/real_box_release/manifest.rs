use super::*;

pub(super) fn release_manifest_template(media_type: &str) -> String {
    format!(
        concat!(
            "agent_release {{\n",
            "  schema = \"a3s.code.agent-release.v1\"\n",
            "  protocol = \"a3s.code.agent.v1\"\n",
            "  artifact {{ digest = \"sha256:1111111111111111111111111111111111111111111111111111111111111111\" media_type = \"{}\" }}\n",
            "  entrypoint {{ command = \"/usr/bin/a3s\" args = [\"code\", \"harness\", \"--manifest\", \"/app/.a3s/asset.acl\"] }}\n",
            "  health {{ transport = \"http\" port = 8080 readiness_path = \"/health/ready\" liveness_path = \"/health/live\" shutdown_grace_seconds = 30 }}\n",
            "  storage {{ workspace = \"ephemeral\" cache = \"ephemeral\" persistent_data = \"none\" }}\n",
            "  capability \"runtime.service\" {{ level = 1 }}\n",
            "  capability \"secrets.external\" {{ level = 1 }}\n",
            "  capability \"workspace.local\" {{ level = 1 }}\n",
            "  secret \"provider-api-key\" {{ target = \"environment\" destination = \"PROVIDER_API_KEY\" }}\n",
            "  secret \"signing-key\" {{ target = \"file\" destination = \"/run/secrets/signing-key\" }}\n",
            "  provenance \"source\" {{ uri = \"urn:a3s:source:template\" digest = \"sha256:2222222222222222222222222222222222222222222222222222222222222222\" }}\n",
            "  provenance \"builder\" {{ uri = \"urn:a3s:builder:template\" digest = \"sha256:4444444444444444444444444444444444444444444444444444444444444444\" }}\n",
            "}}\n"
        ),
        media_type
    )
}

pub(super) fn published_agent_runtime_template(
    provider_secret_id: SecretId,
    signing_secret_id: SecretId,
) -> SourceWorkloadTemplate {
    let mut template = agent_runtime_template();
    template.secrets = vec![
        SecretBinding {
            name: "provider-api-key".into(),
            secret_id: provider_secret_id,
            version: 1,
            target: SecretBindingTarget::Environment {
                variable: "PROVIDER_API_KEY".into(),
            },
        },
        SecretBinding {
            name: "signing-key".into(),
            secret_id: signing_secret_id,
            version: 1,
            target: SecretBindingTarget::File {
                path: "/run/secrets/signing-key".into(),
                mode: 0o400,
            },
        },
    ];
    template
}

pub(super) fn verify_projected_runtime(
    spec: &RuntimeUnitSpec,
    image: &ArtifactRef,
    manifest_artifact: &ArtifactRef,
    manifest_identity: &str,
    provider_secret_reference: CloudSecretReference,
    signing_secret_reference: CloudSecretReference,
) -> TestResult {
    if &spec.artifact != image
        || spec.process.command != [AGENT_RELEASE_ENTRYPOINT_COMMAND_V1]
        || !spec
            .process
            .args
            .iter()
            .map(String::as_str)
            .eq(AGENT_RELEASE_ENTRYPOINT_ARGS_V1)
        || spec.generation != 1
    {
        return Err(invalid("Agent Workload changed its release-owned Runtime intent").into());
    }
    let Some(health) = &spec.health else {
        return Err(invalid("Agent Workload omitted its readiness probe").into());
    };
    let Some(lifecycle) = &spec.service_lifecycle else {
        return Err(invalid("Agent Workload omitted its liveness policy").into());
    };
    if !matches!(
        &health.probe,
        HealthProbe::Http { path, .. } if path == "/health/ready"
    ) || !matches!(
        &lifecycle.liveness.probe,
        HealthProbe::Http { path, .. } if path == "/health/live"
    ) {
        return Err(invalid("Agent Workload changed its manifest-owned health policy").into());
    }
    let manifest_mount = spec
        .mounts
        .iter()
        .find(|mount| mount.name == "agent-release-manifest")
        .ok_or_else(|| invalid("Agent Workload omitted its release manifest mount"))?;
    if manifest_mount.target != "/app/.a3s"
        || !manifest_mount.read_only
        || !matches!(
            &manifest_mount.source,
            RuntimeMountSource::Artifact { artifact } if artifact == manifest_artifact
        )
    {
        return Err(invalid("Agent Workload changed its exact manifest Artifact mount").into());
    }
    let provider_secret = spec
        .secrets
        .iter()
        .find(|secret| secret.name == "provider-api-key")
        .ok_or_else(|| invalid("Agent Workload omitted its provider Secret"))?;
    let signing_secret = spec
        .secrets
        .iter()
        .find(|secret| secret.name == "signing-key")
        .ok_or_else(|| invalid("Agent Workload omitted its signing Secret"))?;
    if spec.secrets.len() != 2
        || provider_secret.reference != provider_secret_reference.to_string()
        || !matches!(
            &provider_secret.target,
            SecretTarget::Environment { variable } if variable == "PROVIDER_API_KEY"
        )
        || signing_secret.reference != signing_secret_reference.to_string()
        || !matches!(
            &signing_secret.target,
            SecretTarget::File { path, mode }
                if path == "/run/secrets/signing-key" && *mode == 0o400
        )
    {
        return Err(invalid("Agent Workload changed its exact release Secret bindings").into());
    }
    Sha256Digest::parse(manifest_identity)?;
    Ok(())
}

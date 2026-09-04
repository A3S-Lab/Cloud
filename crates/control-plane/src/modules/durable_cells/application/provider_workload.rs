use super::workload_port::DurableCellWorkloadTemplate;
use crate::modules::data::{
    ObjectNamespaceCredentialBinding, ObjectNamespaceCredentialBindingSpec,
    ObjectNamespaceProviderProfile,
};
use crate::modules::durable_cells::domain::{
    DurableCellProjectionIdentity, DurableCellProviderBinding, DurableCellProviderHealthProjection,
    DurableCellProviderPortProjection, DurableCellProviderWorkloadProjection,
    DurableCellPublisherProfile, DurableCellServiceProfile, DurableCellStorageBinding,
    DURABLE_CELL_MANAGED_OWNER_KIND,
};
use crate::modules::shared_kernel::domain::{
    SecretVersionReference, Sha256Digest, StorageNamespaceId,
};
use crate::modules::workloads::{
    ManagedOwnerKind, ManagedOwnerReference, SecretBinding, SecretBindingTarget, ServiceProcess,
    ServiceTemplate, WorkloadRevision,
};
use a3s_cloud_contracts::{OCI_IMAGE_INDEX_MEDIA_TYPE, OCI_IMAGE_MANIFEST_MEDIA_TYPE};

const ACCESS_KEY_BINDING: &str = "s0-access-key-id";
const SECRET_ACCESS_KEY_BINDING: &str = "s0-secret-access-key";
const SESSION_TOKEN_BINDING: &str = "s0-session-token";
const CELLD_IDLE_EVICT_ENVIRONMENT: &str = "CELLD_IDLE_EVICT_S";
const CELLD_IDLE_EVICT_SECONDS: &str = "30";

/// Translate the Workloads-owned aggregate into the minimal immutable view
/// admitted by Durable Cells Domain. This is the sole owner-model crossing;
/// the Domain never receives a Workload aggregate or Service template.
pub(crate) fn project_durable_cell_provider_workload(
    revision: &WorkloadRevision,
) -> Result<DurableCellProviderWorkloadProjection, String> {
    let template = revision.resolved_template()?;
    template.validate()?;
    let projection = DurableCellProviderWorkloadProjection {
        workload_id: revision.workload_id,
        workload_revision_id: revision.id,
        workload_generation: revision.generation,
        service_template_digest: Sha256Digest::parse(template.digest()?)?,
        provider_artifact_digest: Sha256Digest::parse(&template.artifact.digest)?,
        ports: template
            .ports
            .iter()
            .map(|port| DurableCellProviderPortProjection {
                name: port.name.clone(),
                container_port: port.container_port,
            })
            .collect(),
        health: template
            .health
            .as_ref()
            .map(|health| DurableCellProviderHealthProjection {
                port_name: health.port_name.clone(),
                path: health.path.clone(),
            }),
    };
    projection.validate()?;
    Ok(projection)
}

pub(crate) fn validate_durable_cell_provider_workload_binding(
    binding: &DurableCellProviderBinding,
    profile: &DurableCellServiceProfile,
    revision: &WorkloadRevision,
) -> Result<(), String> {
    validate_durable_cell_provider_workload_projection(
        binding,
        profile,
        &project_durable_cell_provider_workload(revision)?,
    )
}

/// Validates the immutable Workloads projection after it has crossed the
/// consumer-owned port. This keeps the Durable Cells domain policy independent
/// of the Workloads aggregate while retaining the same exact projection
/// checks used during initial admission.
pub(crate) fn validate_durable_cell_provider_workload_projection(
    binding: &DurableCellProviderBinding,
    profile: &DurableCellServiceProfile,
    projection: &DurableCellProviderWorkloadProjection,
) -> Result<(), String> {
    binding.validate_workload_projection(profile, projection)
}

/// Compile the Durable Cells owner identity into Workloads' generic managed
/// owner value at the Application boundary. The Domain owns the source facts,
/// while Workloads owns the target vocabulary.
pub(crate) fn durable_cell_managed_owner_reference(
    projection: &DurableCellProjectionIdentity,
) -> Result<ManagedOwnerReference, String> {
    projection.validate()?;
    ManagedOwnerReference::new(
        ManagedOwnerKind::parse(DURABLE_CELL_MANAGED_OWNER_KIND)?,
        projection.application_id.as_uuid(),
        projection.application_revision_number,
        projection.application_definition_digest.as_str(),
    )
}

/// The sole translation from the reviewed celld/S0 profiles into the
/// long-running Workloads-owned Service process.
///
/// CELL0.5 is intentionally single-replica. The internal listener therefore
/// advertises loopback while still binding the Runtime port. CELL0.6 must
/// replace that value with an existing Fleet/private-network identity before
/// multi-node placement is admitted; it must not add another Service
/// lifecycle or provider configuration authority.
pub fn compose_pinned_celld_service_process(
    provider_profile: &ObjectNamespaceProviderProfile,
    storage_namespace_id: StorageNamespaceId,
    public_container_port: u16,
    internal_container_port: u16,
    publisher: &DurableCellPublisherProfile,
) -> Result<ServiceProcess, String> {
    provider_profile.validate()?;
    publisher.validate()?;
    if provider_profile.spec().virtual_hosted_style {
        return Err("celld v0.2.1 Service requires path-style S0 addressing".into());
    }
    if public_container_port == 0
        || internal_container_port == 0
        || public_container_port == internal_container_port
    {
        return Err("celld Service requires distinct nonzero public and internal ports".into());
    }
    let namespace_prefix = provider_profile.namespace_prefix(storage_namespace_id)?;
    Ok(ServiceProcess {
        command: publisher.command().to_vec(),
        args: vec![
            "--bucket".into(),
            format!(
                "s3://{}/{}",
                provider_profile.spec().bucket,
                namespace_prefix
            ),
            "--endpoint".into(),
            provider_profile.spec().endpoint.clone(),
            "--region".into(),
            provider_profile.spec().region.clone(),
            "--listen".into(),
            format!("0.0.0.0:{public_container_port}"),
            "--internal-listen".into(),
            format!("0.0.0.0:{internal_container_port}"),
            "--advertise".into(),
            format!("127.0.0.1:{internal_container_port}"),
        ],
        working_directory: Some("/".into()),
        // celld leaves idle eviction disabled unless this provider-owned
        // policy is set, while the canonical Service profile requires the
        // behavior. Keeping the exact value in this sole adapter also means
        // an input cannot disable celld's default RPO=0 output gate or add an
        // unowned host cache path.
        environment: std::collections::BTreeMap::from([(
            CELLD_IDLE_EVICT_ENVIRONMENT.into(),
            CELLD_IDLE_EVICT_SECONDS.into(),
        )]),
    })
}

/// Validates the complete non-secret Service projection against the exact S0
/// namespace used by publication. This is reused by initial deployment and by
/// publication recovery so a persisted Workload cannot publish successfully
/// and then start celld against a different bucket or prefix. The pinned Box
/// provider does not advertise ephemeral-storage control, so this adapter also
/// rejects rather than silently accepting that unsupported resource promise.
pub(super) fn validate_pinned_celld_service_projection(
    provider_profile: &ObjectNamespaceProviderProfile,
    storage_namespace_id: StorageNamespaceId,
    service_profile: &DurableCellServiceProfile,
    template: &ServiceTemplate,
    publisher: &DurableCellPublisherProfile,
) -> Result<(), String> {
    provider_profile.validate()?;
    template.validate()?;
    publisher.validate()?;
    let reviewed_service_profile = DurableCellServiceProfile::pinned_celld_v0_2_1()?;
    if service_profile != &reviewed_service_profile {
        return Err(
            "pinned celld v0.2.1 requires its exact reviewed Durable Cell Service profile".into(),
        );
    }
    if template.artifact.uri != publisher.image_uri()
        || template.artifact.digest != publisher.image_digest().as_str()
        || !matches!(
            template.artifact.media_type.as_str(),
            OCI_IMAGE_MANIFEST_MEDIA_TYPE | OCI_IMAGE_INDEX_MEDIA_TYPE
        )
    {
        return Err("Durable Cell provider Workload is not the exact pinned celld image".into());
    }
    if template.resources.ephemeral_storage_bytes.is_some() {
        return Err(
            "pinned celld Service cannot request unsupported Box ephemeral-storage control".into(),
        );
    }
    let profile = service_profile.spec();
    let public = template
        .ports
        .iter()
        .find(|port| port.name == profile.public_runtime_port)
        .ok_or_else(|| {
            "Durable Cell provider Service omitted its public Runtime port".to_owned()
        })?;
    let internal = template
        .ports
        .iter()
        .find(|port| port.name == profile.internal_runtime_port)
        .ok_or_else(|| {
            "Durable Cell provider Service omitted its internal Runtime port".to_owned()
        })?;
    let expected = compose_pinned_celld_service_process(
        provider_profile,
        storage_namespace_id,
        public.container_port,
        internal.container_port,
        publisher,
    )?;
    if template.process != expected {
        return Err(
            "Durable Cell provider process does not match the exact reviewed celld/S0 adapter"
                .into(),
        );
    }
    Ok(())
}

/// Validates one complete profile-bound provider Workload without resolving
/// Secret material. Workloads remains the Service owner and Secrets remains
/// the only materialization authority.
pub(super) fn validate_pinned_celld_provider_workload(
    credentials: &ObjectNamespaceCredentialBinding,
    provider_profile: &ObjectNamespaceProviderProfile,
    service_profile: &DurableCellServiceProfile,
    template: &ServiceTemplate,
    publisher: &DurableCellPublisherProfile,
) -> Result<(), String> {
    credentials.validate_provider_profile(provider_profile)?;
    validate_pinned_celld_service_projection(
        provider_profile,
        credentials.spec().namespace_id,
        service_profile,
        template,
        publisher,
    )?;
    validate_publisher_storage_credentials(credentials, template, publisher)
}

/// Checks the exact Secret surface exposed to the pinned celld adapter.
///
/// The deployment binding owns which Secret versions are authoritative, while
/// the publisher profile owns the provider-facing environment variable names.
/// Keeping this translation here prevents the Service and pre-start Task from
/// inventing separate credential mappings.
pub(super) fn validate_publisher_storage_credentials(
    credentials: &ObjectNamespaceCredentialBinding,
    template: &ServiceTemplate,
    publisher: &DurableCellPublisherProfile,
) -> Result<(), String> {
    credentials.validate()?;
    validate_bindings(
        &template.secrets,
        publisher,
        Some([
            Some(credentials.spec().access_key_id),
            Some(credentials.spec().secret_access_key),
            credentials.spec().session_token,
        ]),
    )
}

/// Revalidates a persisted Workload before projecting its Secrets into the
/// publication Task. The credential digest remains owned by S0; this check is
/// only the immutable, plaintext-free provider adapter shape.
pub(super) fn validate_publisher_secret_targets(
    template: &ServiceTemplate,
    publisher: &DurableCellPublisherProfile,
) -> Result<(), String> {
    validate_bindings(&template.secrets, publisher, None)
}

/// Decodes and validates the opaque Workloads template only at the owner
/// translation helper. Durable Cells application code receives the resulting
/// media type, but never imports or reconstructs the Workloads model.
pub(super) fn validate_pinned_celld_service_template_payload(
    provider_profile: &ObjectNamespaceProviderProfile,
    storage_namespace_id: StorageNamespaceId,
    service_profile: &DurableCellServiceProfile,
    payload: &DurableCellWorkloadTemplate,
    publisher: &DurableCellPublisherProfile,
) -> Result<String, String> {
    let template = serde_json::from_slice::<ServiceTemplate>(payload.bytes())
        .map_err(|error| format!("invalid opaque Workloads Service template: {error}"))?;
    let digest = Sha256Digest::parse(template.digest()?)?;
    if digest != *payload.digest() {
        return Err("opaque Workloads Service template digest changed".into());
    }
    validate_pinned_celld_service_projection(
        provider_profile,
        storage_namespace_id,
        service_profile,
        &template,
        publisher,
    )?;
    validate_publisher_secret_targets(&template, publisher)?;
    Ok(template.artifact.media_type)
}

/// Reconstructs the exact plaintext-free S0 credential binding from the
/// immutable Workload revision and its Durable Cell correlation. Secret
/// material remains in Secrets; the stored S0 digest rejects substituted
/// references, scope, generation, or provider identity.
pub(crate) fn restore_publisher_storage_credentials(
    storage: &DurableCellStorageBinding,
    template: &ServiceTemplate,
    publisher: &DurableCellPublisherProfile,
) -> Result<ObjectNamespaceCredentialBinding, String> {
    storage.validate()?;
    validate_publisher_secret_targets(template, publisher)?;
    let reference = |name: &str| {
        template
            .secrets
            .iter()
            .find(|binding| binding.name == name)
            .map(|binding| SecretVersionReference::new(binding.secret_id, binding.version))
            .transpose()
    };
    let credentials = ObjectNamespaceCredentialBinding::restore(
        ObjectNamespaceCredentialBindingSpec {
            organization_id: storage.organization_id,
            project_id: storage.project_id,
            environment_id: storage.environment_id,
            namespace_id: storage.storage_namespace_id,
            generation: storage.credential_binding_generation,
            provider_profile_digest: storage.provider_profile_digest.clone(),
            access_key_id: reference(ACCESS_KEY_BINDING)?.ok_or_else(|| {
                "Durable Cell provider template omitted its S0 access-key reference".to_owned()
            })?,
            secret_access_key: reference(SECRET_ACCESS_KEY_BINDING)?.ok_or_else(|| {
                "Durable Cell provider template omitted its S0 secret-key reference".to_owned()
            })?,
            session_token: reference(SESSION_TOKEN_BINDING)?,
        },
        storage.credential_binding_digest.as_str(),
    )?;
    validate_publisher_storage_credentials(&credentials, template, publisher)?;
    Ok(credentials)
}

fn validate_bindings(
    bindings: &[SecretBinding],
    publisher: &DurableCellPublisherProfile,
    references: Option<[Option<SecretVersionReference>; 3]>,
) -> Result<(), String> {
    publisher.validate()?;
    let session_reference = references.as_ref().and_then(|values| values[2]);
    let session_required = session_reference.is_some()
        || references.is_none()
            && bindings
                .iter()
                .any(|binding| binding.name == SESSION_TOKEN_BINDING);
    let expected = [
        (
            ACCESS_KEY_BINDING,
            publisher.access_key_environment(),
            references.as_ref().and_then(|values| values[0]),
            true,
        ),
        (
            SECRET_ACCESS_KEY_BINDING,
            publisher.secret_access_key_environment(),
            references.as_ref().and_then(|values| values[1]),
            true,
        ),
        (
            SESSION_TOKEN_BINDING,
            publisher.session_token_environment(),
            session_reference,
            session_required,
        ),
    ];
    let required_count = expected
        .iter()
        .filter(|(_, _, _, required)| *required)
        .count();
    if bindings.len() != required_count {
        return Err(
            "Durable Cell provider template must expose only the exact S0 credential bindings"
                .into(),
        );
    }
    for (name, variable, reference, required) in expected {
        let binding = bindings.iter().find(|binding| binding.name == name);
        let Some(binding) = binding else {
            if required {
                return Err(format!(
                    "Durable Cell provider template omitted the {name} credential binding"
                ));
            }
            continue;
        };
        if !required
            || reference.is_some_and(|reference| {
                binding.secret_id != reference.secret_id || binding.version != reference.version
            })
            || !matches!(
                &binding.target,
                SecretBindingTarget::Environment { variable: target } if target == variable
            )
        {
            return Err(format!(
                "Durable Cell provider credential binding {name} changed its exact Secret or environment target"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::data::{
        ObjectNamespaceCredentialBindingSpec, ObjectNamespaceProviderProfile,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, OrganizationId, ProjectId, SecretId, StorageNamespaceId,
    };
    use crate::modules::workloads::{HttpHealthCheck, OciArtifact, ServicePort, ServiceResources};
    fn provider_profile() -> ObjectNamespaceProviderProfile {
        ObjectNamespaceProviderProfile::parse_acl(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/s0.1/object-namespace-provider-profile.acl"
        )))
        .expect("S0 profile")
    }

    fn binding(profile: &ObjectNamespaceProviderProfile) -> ObjectNamespaceCredentialBinding {
        ObjectNamespaceCredentialBinding::from_spec(ObjectNamespaceCredentialBindingSpec {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            namespace_id: StorageNamespaceId::new(),
            generation: 1,
            provider_profile_digest: profile.digest().clone(),
            access_key_id: SecretVersionReference::new(SecretId::new(), 1).expect("access key"),
            secret_access_key: SecretVersionReference::new(SecretId::new(), 2).expect("secret key"),
            session_token: None,
        })
        .expect("binding")
    }

    fn template(
        credentials: &ObjectNamespaceCredentialBinding,
        provider_profile: &ObjectNamespaceProviderProfile,
        service_profile: &DurableCellServiceProfile,
        publisher: &DurableCellPublisherProfile,
    ) -> ServiceTemplate {
        ServiceTemplate {
            artifact: OciArtifact {
                uri: publisher.image_uri().into(),
                digest: publisher.image_digest().to_string(),
                media_type: OCI_IMAGE_INDEX_MEDIA_TYPE.into(),
            },
            process: compose_pinned_celld_service_process(
                provider_profile,
                credentials.spec().namespace_id,
                8080,
                8081,
                publisher,
            )
            .expect("celld Service process"),
            secrets: vec![
                secret(
                    ACCESS_KEY_BINDING,
                    credentials.spec().access_key_id,
                    "AWS_ACCESS_KEY_ID",
                ),
                secret(
                    SECRET_ACCESS_KEY_BINDING,
                    credentials.spec().secret_access_key,
                    "AWS_SECRET_ACCESS_KEY",
                ),
            ],
            resources: ServiceResources {
                cpu_millis: 1_000,
                memory_bytes: 512 * 1024 * 1024,
                pids: 256,
                ephemeral_storage_bytes: None,
            },
            ports: vec![
                ServicePort {
                    name: service_profile.spec().public_runtime_port.clone(),
                    container_port: 8080,
                },
                ServicePort {
                    name: service_profile.spec().internal_runtime_port.clone(),
                    container_port: 8081,
                },
            ],
            health: Some(HttpHealthCheck {
                port_name: service_profile.spec().public_runtime_port.clone(),
                path: service_profile.spec().health_path.clone(),
                interval_ms: 1_000,
                timeout_ms: 500,
                healthy_threshold: 1,
                unhealthy_threshold: 3,
                stabilization_window_ms: 5_000,
            }),
        }
    }

    fn secret(name: &str, reference: SecretVersionReference, variable: &str) -> SecretBinding {
        SecretBinding {
            name: name.into(),
            secret_id: reference.secret_id,
            version: reference.version,
            target: SecretBindingTarget::Environment {
                variable: variable.into(),
            },
        }
    }

    #[test]
    fn accepts_only_the_exact_s0_backed_celld_service_projection() {
        let provider_profile = provider_profile();
        let service_profile =
            DurableCellServiceProfile::pinned_celld_v0_2_1().expect("pinned Service profile");
        let credentials = binding(&provider_profile);
        let publisher = DurableCellPublisherProfile::pinned_celld_v0_2_1().expect("publisher");
        let template = template(
            &credentials,
            &provider_profile,
            &service_profile,
            &publisher,
        );
        validate_pinned_celld_provider_workload(
            &credentials,
            &provider_profile,
            &service_profile,
            &template,
            &publisher,
        )
        .expect("exact celld Service projection");
        validate_publisher_storage_credentials(&credentials, &template, &publisher)
            .expect("exact credentials");
        validate_publisher_secret_targets(&template, &publisher).expect("exact targets");

        assert_eq!(template.process.args[0], "--bucket");
        assert_eq!(
            template.process.args[1],
            format!(
                "s3://a3s-durable-cells/a3s/durable-cells/{}",
                credentials.spec().namespace_id
            )
        );
        assert_eq!(
            template.process.args.last().map(String::as_str),
            Some("127.0.0.1:8081")
        );
        assert_eq!(
            template.process.environment,
            std::collections::BTreeMap::from([("CELLD_IDLE_EVICT_S".into(), "30".into(),)])
        );

        let mut wrong_namespace = template.clone();
        wrong_namespace.process.args[1] = "s3://a3s-durable-cells/a3s/durable-cells/foreign".into();
        assert!(validate_pinned_celld_service_projection(
            &provider_profile,
            credentials.spec().namespace_id,
            &service_profile,
            &wrong_namespace,
            &publisher,
        )
        .is_err());

        let mut missing_advertise = template.clone();
        missing_advertise.process.args.truncate(10);
        assert!(validate_pinned_celld_service_projection(
            &provider_profile,
            credentials.spec().namespace_id,
            &service_profile,
            &missing_advertise,
            &publisher,
        )
        .is_err());

        let mut weakened_durability = template.clone();
        weakened_durability
            .process
            .environment
            .insert("CELLD_OUTPUT_GATE".into(), "0".into());
        assert!(validate_pinned_celld_service_projection(
            &provider_profile,
            credentials.spec().namespace_id,
            &service_profile,
            &weakened_durability,
            &publisher,
        )
        .is_err());

        let mut disabled_idle_eviction = template.clone();
        disabled_idle_eviction.process.environment.clear();
        assert!(validate_pinned_celld_service_projection(
            &provider_profile,
            credentials.spec().namespace_id,
            &service_profile,
            &disabled_idle_eviction,
            &publisher,
        )
        .is_err());

        let mut unsupported_storage_control = template.clone();
        unsupported_storage_control
            .resources
            .ephemeral_storage_bytes = Some(512 * 1024 * 1024);
        assert!(validate_pinned_celld_service_projection(
            &provider_profile,
            credentials.spec().namespace_id,
            &service_profile,
            &unsupported_storage_control,
            &publisher,
        )
        .is_err());

        let mut drifted_profile_spec = service_profile.spec().clone();
        drifted_profile_spec.health_path = "/different-health".into();
        let drifted_profile = DurableCellServiceProfile::from_spec(drifted_profile_spec)
            .expect("structurally valid drifted profile");
        assert!(validate_pinned_celld_service_projection(
            &provider_profile,
            credentials.spec().namespace_id,
            &drifted_profile,
            &template,
            &publisher,
        )
        .is_err());

        let mut wrong_target = template.clone();
        wrong_target.secrets[0].target = SecretBindingTarget::Environment {
            variable: "S0_ACCESS_KEY_ID".into(),
        };
        assert!(
            validate_publisher_storage_credentials(&credentials, &wrong_target, &publisher)
                .is_err()
        );

        let mut extra = template;
        extra.secrets.push(secret(
            "unrelated",
            SecretVersionReference::new(SecretId::new(), 1).expect("extra"),
            "UNRELATED",
        ));
        assert!(validate_publisher_secret_targets(&extra, &publisher).is_err());
    }
}

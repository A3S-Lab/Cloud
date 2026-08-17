use crate::modules::data::ObjectNamespaceCredentialBinding;
use crate::modules::durable_cells::domain::DurableCellPublisherProfile;
use crate::modules::shared_kernel::domain::SecretVersionReference;
use crate::modules::workloads::{SecretBinding, SecretBindingTarget, ServiceTemplate};

const ACCESS_KEY_BINDING: &str = "s0-access-key-id";
const SECRET_ACCESS_KEY_BINDING: &str = "s0-secret-access-key";
const SESSION_TOKEN_BINDING: &str = "s0-session-token";

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
    use crate::modules::workloads::{OciArtifact, ServiceProcess, ServiceResources};
    use std::collections::BTreeMap;

    fn binding() -> ObjectNamespaceCredentialBinding {
        let profile = ObjectNamespaceProviderProfile::parse_acl(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/s0.1/object-namespace-provider-profile.acl"
        )))
        .expect("S0 profile");
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

    fn template(credentials: &ObjectNamespaceCredentialBinding) -> ServiceTemplate {
        let digest = format!("sha256:{}", "a".repeat(64));
        ServiceTemplate {
            artifact: OciArtifact {
                uri: format!("oci://example.invalid/celld@{digest}"),
                digest,
                media_type: "application/vnd.oci.image.index.v1+json".into(),
            },
            process: ServiceProcess {
                command: vec!["/usr/local/bin/celld".into()],
                args: Vec::new(),
                working_directory: Some("/".into()),
                environment: BTreeMap::new(),
            },
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
                cpu_millis: 1,
                memory_bytes: 1,
                pids: 1,
                ephemeral_storage_bytes: None,
            },
            ports: Vec::new(),
            health: None,
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
    fn accepts_only_exact_credential_references_and_provider_targets() {
        let credentials = binding();
        let publisher = DurableCellPublisherProfile::pinned_celld_v0_2_1().expect("publisher");
        let template = template(&credentials);
        validate_publisher_storage_credentials(&credentials, &template, &publisher)
            .expect("exact credentials");
        validate_publisher_secret_targets(&template, &publisher).expect("exact targets");

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

use crate::modules::developer_workflows::application::{
    EnsurePreviewEnvironment, IPreviewEnvironmentPort, PreviewEnvironmentBinding,
    PreviewEnvironmentReceipt,
};
use crate::modules::projects::domain::{
    entities::Environment, events::EnvironmentCreated, repositories::IEnvironmentRepository,
    value_objects::EnvironmentName,
};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, IdempotentWrite, RepositoryError};
use async_trait::async_trait;
use std::sync::Arc;

/// Anti-corruption adapter into Projects' existing Environment aggregate and
/// transactional Outbox. It owns no Environment state of its own.
pub struct ProjectsPreviewEnvironmentAdapter {
    environments: Arc<dyn IEnvironmentRepository>,
}

impl ProjectsPreviewEnvironmentAdapter {
    pub fn new(environments: Arc<dyn IEnvironmentRepository>) -> Self {
        Self { environments }
    }
}

#[async_trait]
impl IPreviewEnvironmentPort for ProjectsPreviewEnvironmentAdapter {
    async fn ensure_preview_environment(
        &self,
        request: EnsurePreviewEnvironment,
    ) -> Result<IdempotentWrite<PreviewEnvironmentReceipt>, RepositoryError> {
        request.validate().map_err(RepositoryError::Storage)?;
        let binding = request.binding;
        if let Some(existing) = self
            .environments
            .find(
                binding.organization_id,
                binding.project_id,
                binding.environment_id,
            )
            .await?
        {
            validate_environment(&existing, &binding)?;
            return Ok(IdempotentWrite {
                value: receipt(binding, existing.aggregate_version),
                replayed: true,
            });
        }

        let environment = Environment::create(
            binding.organization_id,
            binding.project_id,
            binding.environment_id,
            EnvironmentName::parse(&binding.name).map_err(RepositoryError::Storage)?,
            binding.created_at,
        );
        let canonical = serde_json::to_vec(&binding).map_err(|error| {
            RepositoryError::Storage(format!(
                "Preview Environment handoff could not be canonicalized: {error}"
            ))
        })?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/projects/{}/preview-environments/{}",
                binding.organization_id, binding.project_id, binding.preview_id
            ),
            "ensure",
            &canonical,
        )
        .map_err(RepositoryError::Storage)?;
        let mut event = EnvironmentCreated::envelope(&environment, request.correlation_id)
            .map_err(|error| {
                RepositoryError::Storage(format!(
                    "Preview Environment event could not be encoded: {error}"
                ))
            })?;
        event.causation_id = Some(request.causation_id);
        let write = match self
            .environments
            .create(environment, event, idempotency)
            .await
        {
            Ok(write) => write,
            Err(conflict @ RepositoryError::Conflict(_)) => {
                let Some(existing) = self
                    .environments
                    .find(
                        binding.organization_id,
                        binding.project_id,
                        binding.environment_id,
                    )
                    .await?
                else {
                    return Err(conflict);
                };
                validate_environment(&existing, &binding)?;
                return Ok(IdempotentWrite {
                    value: receipt(binding, existing.aggregate_version),
                    replayed: true,
                });
            }
            Err(error) => return Err(error),
        };
        validate_environment(&write.value, &binding)?;
        Ok(IdempotentWrite {
            value: receipt(binding, write.value.aggregate_version),
            replayed: write.replayed,
        })
    }
}

fn validate_environment(
    environment: &Environment,
    binding: &PreviewEnvironmentBinding,
) -> Result<(), RepositoryError> {
    if environment.organization_id != binding.organization_id
        || environment.project_id != binding.project_id
        || environment.id != binding.environment_id
        || environment.name.as_str() != binding.name
        || environment.aggregate_version != 1
        || environment.created_at != binding.created_at
    {
        return Err(RepositoryError::Conflict(
            "Preview Environment identity is already bound to different Projects state".into(),
        ));
    }
    Ok(())
}

fn receipt(
    binding: PreviewEnvironmentBinding,
    environment_aggregate_version: u64,
) -> PreviewEnvironmentReceipt {
    PreviewEnvironmentReceipt {
        binding,
        environment_aggregate_version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::developer_workflows::domain::PullRequestPreview;
    use crate::modules::projects::infrastructure::persistence::InMemoryProjectsRepository;
    use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId, PullRequestPreviewId};
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    #[tokio::test]
    async fn creates_one_projects_environment_and_replays_without_another_event() {
        let projects = Arc::new(InMemoryProjectsRepository::default());
        let adapter = ProjectsPreviewEnvironmentAdapter::new(projects.clone());
        let request = request();
        let first = adapter
            .ensure_preview_environment(request.clone())
            .await
            .expect("first Environment handoff");
        assert!(!first.replayed);
        first
            .value
            .validate_for(&request.binding)
            .expect("exact receipt");

        let replay = adapter
            .ensure_preview_environment(request.clone())
            .await
            .expect("replayed Environment handoff");
        assert!(replay.replayed);
        assert_eq!(projects.outbox_events().await.len(), 1);
        assert_eq!(
            projects
                .find(
                    request.binding.organization_id,
                    request.binding.project_id,
                    request.binding.environment_id,
                )
                .await
                .expect("Projects read")
                .expect("Environment")
                .name
                .as_str(),
            request.binding.name
        );
    }

    #[tokio::test]
    async fn rejects_a_preclaimed_deterministic_environment_identity() {
        let projects = Arc::new(InMemoryProjectsRepository::default());
        let adapter = ProjectsPreviewEnvironmentAdapter::new(projects.clone());
        let request = request();
        let preclaimed = Environment::create(
            request.binding.organization_id,
            request.binding.project_id,
            request.binding.environment_id,
            EnvironmentName::parse("not-the-preview").expect("name"),
            request.binding.created_at,
        );
        projects
            .create(
                preclaimed.clone(),
                EnvironmentCreated::envelope(&preclaimed, Uuid::now_v7()).expect("event"),
                IdempotencyRequest::new("tests/preclaimed", "create", b"preclaimed")
                    .expect("idempotency"),
            )
            .await
            .expect("preclaim");
        assert!(matches!(
            adapter.ensure_preview_environment(request).await,
            Err(RepositoryError::Conflict(message))
                if message.contains("different Projects state")
        ));
    }

    fn request() -> EnsurePreviewEnvironment {
        let preview_id = PullRequestPreviewId::new();
        let pull_request_number = 42;
        EnsurePreviewEnvironment {
            binding: PreviewEnvironmentBinding {
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                preview_id,
                environment_id: PullRequestPreview::environment_id_for(preview_id),
                pull_request_number,
                name: PullRequestPreview::environment_name_for(preview_id, pull_request_number)
                    .expect("name"),
                created_at: Utc
                    .with_ymd_and_hms(2026, 8, 26, 4, 30, 0)
                    .single()
                    .expect("timestamp"),
            },
            correlation_id: Uuid::now_v7(),
            causation_id: Uuid::now_v7(),
        }
    }
}

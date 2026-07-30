use super::InMemoryEdgeRepository;
use crate::modules::edge::domain::repositories::IMcpCredentialRepository;
use crate::modules::edge::domain::McpCredential;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, McpCredentialId, OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;

#[async_trait]
impl IMcpCredentialRepository for InMemoryEdgeRepository {
    async fn create_mcp_credential(
        &self,
        credential: McpCredential,
    ) -> Result<McpCredential, RepositoryError> {
        if credential.generation() != 1
            || credential.aggregate_version() != 1
            || credential.created_at() != credential.updated_at()
            || credential.revoked_at().is_some()
        {
            return Err(RepositoryError::Conflict(
                "new MCP credential is not at its initial generation".into(),
            ));
        }
        let mut state = self.state.write().await;
        if state.mcp_credentials.contains_key(&credential.id)
            || state
                .mcp_credential_prefixes
                .contains_key(credential.prefix())
        {
            return Err(RepositoryError::Conflict(
                "MCP credential identity or lookup prefix is already in use".into(),
            ));
        }
        state
            .mcp_credential_prefixes
            .insert(credential.prefix().to_owned(), credential.id);
        state
            .mcp_credentials
            .insert(credential.id, credential.clone());
        Ok(credential)
    }

    async fn update_mcp_credential(
        &self,
        credential: McpCredential,
        expected_aggregate_version: u64,
    ) -> Result<McpCredential, RepositoryError> {
        let mut state = self.state.write().await;
        let existing = state
            .mcp_credentials
            .get(&credential.id)
            .filter(|existing| existing.organization_id == credential.organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        credential
            .validate_transition_from(&existing, expected_aggregate_version)
            .map_err(RepositoryError::Conflict)?;
        if state
            .mcp_credential_prefixes
            .get(credential.prefix())
            .is_some_and(|id| *id != credential.id)
        {
            return Err(RepositoryError::Conflict(
                "MCP credential lookup prefix is already in use".into(),
            ));
        }
        state.mcp_credential_prefixes.remove(existing.prefix());
        state
            .mcp_credential_prefixes
            .insert(credential.prefix().to_owned(), credential.id);
        state
            .mcp_credentials
            .insert(credential.id, credential.clone());
        Ok(credential)
    }

    async fn find_mcp_credential(
        &self,
        organization_id: OrganizationId,
        credential_id: McpCredentialId,
    ) -> Result<Option<McpCredential>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .mcp_credentials
            .get(&credential_id)
            .filter(|credential| credential.organization_id == organization_id)
            .cloned())
    }

    async fn list_mcp_credentials(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<McpCredential>, RepositoryError> {
        let mut credentials = self
            .state
            .read()
            .await
            .mcp_credentials
            .values()
            .filter(|credential| {
                credential.organization_id == organization_id
                    && credential.project_id == project_id
                    && credential.environment_id == environment_id
            })
            .cloned()
            .collect::<Vec<_>>();
        credentials.sort_by_key(|credential| (credential.created_at(), credential.id));
        Ok(credentials)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const ROTATED_VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0)
            .single()
            .expect("time")
    }

    fn credential(
        id: McpCredentialId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        prefix: &str,
    ) -> McpCredential {
        McpCredential::issue(
            id,
            organization_id,
            project_id,
            environment_id,
            prefix,
            VERIFIER,
            now() + Duration::days(30),
            now(),
        )
        .expect("credential")
    }

    #[tokio::test]
    async fn persists_rotation_revocation_and_tenant_non_disclosure() {
        let repository = InMemoryEdgeRepository::new();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let mut stored = repository
            .create_mcp_credential(credential(
                McpCredentialId::new(),
                organization_id,
                project_id,
                environment_id,
                "a3s_mcp_abc12345def67890",
            ))
            .await
            .expect("create");
        assert_eq!(
            repository
                .find_mcp_credential(organization_id, stored.id)
                .await
                .expect("find"),
            Some(stored.clone())
        );
        assert_eq!(
            repository
                .find_mcp_credential(OrganizationId::new(), stored.id)
                .await
                .expect("tenant non-disclosure"),
            None
        );

        stored
            .rotate(
                "a3s_mcp_def67890abc12345",
                ROTATED_VERIFIER,
                now() + Duration::days(60),
                now() + Duration::minutes(1),
            )
            .expect("rotate");
        stored = repository
            .update_mcp_credential(stored, 1)
            .await
            .expect("persist rotation");
        assert_eq!(stored.generation(), 2);
        assert!(stored.revoke(now() + Duration::minutes(2)).expect("revoke"));
        stored = repository
            .update_mcp_credential(stored, 2)
            .await
            .expect("persist revocation");
        assert_eq!(stored.aggregate_version(), 3);
        assert_eq!(
            repository
                .list_mcp_credentials(organization_id, project_id, environment_id)
                .await
                .expect("list"),
            vec![stored]
        );
    }

    #[tokio::test]
    async fn rejects_prefix_collisions_and_stale_updates() {
        let repository = InMemoryEdgeRepository::new();
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let first = repository
            .create_mcp_credential(credential(
                McpCredentialId::new(),
                organization_id,
                project_id,
                environment_id,
                "a3s_mcp_abc12345def67890",
            ))
            .await
            .expect("first");
        assert!(matches!(
            repository
                .create_mcp_credential(credential(
                    McpCredentialId::new(),
                    organization_id,
                    project_id,
                    environment_id,
                    "a3s_mcp_abc12345def67890",
                ))
                .await,
            Err(RepositoryError::Conflict(_))
        ));

        let mut rotated = first.clone();
        rotated
            .rotate(
                "a3s_mcp_def67890abc12345",
                ROTATED_VERIFIER,
                now() + Duration::days(60),
                now() + Duration::minutes(1),
            )
            .expect("rotate");
        repository
            .update_mcp_credential(rotated.clone(), 1)
            .await
            .expect("update");
        assert!(matches!(
            repository.update_mcp_credential(rotated, 1).await,
            Err(RepositoryError::Conflict(_))
        ));
    }
}

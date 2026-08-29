use super::{
    AcceptPlatformRolePolicy, ChangePlatformRoleBinding, CreatePlatformRoleBinding,
    RevokePlatformRoleBinding,
};
use crate::modules::identity::application::privileged_management::{
    deterministic_id, idempotency, installation_id, not_found,
};
use crate::modules::identity::application::{
    PlatformRoleBindingMutationResult, PlatformRolePolicyMutationResult,
};
use crate::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, PlatformRoleBinding,
};
use crate::modules::identity::domain::repositories::{
    AcceptPlatformRolePolicyRevisionWrite, ChangePlatformRoleBindingWrite,
    CreatePlatformRoleBindingWrite, IIdentityBootstrapRepository, IPlatformRbacRepository,
    RevokePlatformRoleBindingWrite,
};
use crate::modules::identity::domain::value_objects::{PlatformRole, PlatformRolePolicyContract};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::PlatformRoleBindingId;
use a3s_boot::{CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct AcceptPlatformRolePolicyHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn IPlatformRbacRepository>,
}

impl AcceptPlatformRolePolicyHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn IPlatformRbacRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl CommandHandler<AcceptPlatformRolePolicy> for AcceptPlatformRolePolicyHandler {
    fn execute(
        &self,
        command: AcceptPlatformRolePolicy,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<PlatformRolePolicyMutationResult>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let contract = match PlatformRolePolicyContract::parse_acl(&command.canonical_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            if contract.spec().installation_id != installation_id {
                return Ok(Err(ApplicationError::Invalid(
                    "platform role policy crossed the canonical Installation boundary".into(),
                )));
            }
            let revision = match AcceptedPlatformRolePolicyRevision::accept(
                contract,
                command.revision_number,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let idempotency = match idempotency(
                "installation/platform-role-policy".into(),
                command.idempotency_key,
                &serde_json::json!({
                    "canonicalAcl": revision.contract.canonical_acl(),
                    "revisionNumber": revision.revision_number,
                    "expectedCurrentRevisionId": command.expected_current_revision_id,
                }),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let result = match repository
                .accept_platform_role_policy_revision(AcceptPlatformRolePolicyRevisionWrite {
                    revision,
                    expected_current_revision_id: command.expected_current_revision_id,
                    actor_principal_id: command.actor_principal_id,
                    credential_id: command.credential_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(PlatformRolePolicyMutationResult {
                policy: result.value,
                replayed: result.replayed,
            }))
        })
    }
}

pub struct CreatePlatformRoleBindingHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn IPlatformRbacRepository>,
}

impl CreatePlatformRoleBindingHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn IPlatformRbacRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl CommandHandler<CreatePlatformRoleBinding> for CreatePlatformRoleBindingHandler {
    fn execute(
        &self,
        command: CreatePlatformRoleBinding,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<PlatformRoleBindingMutationResult>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let role = match PlatformRole::parse(&command.role) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let policy = match repository
                .find_platform_role_policy_revision(
                    installation_id,
                    command.expected_policy_revision_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) => return Ok(Err(not_found("platform role policy revision"))),
                Err(error) => return Ok(Err(error.into())),
            };
            let idempotency = match idempotency(
                "installation/platform-role-bindings".into(),
                command.idempotency_key,
                &serde_json::json!({
                    "principalId": command.principal_id,
                    "role": role.as_str(),
                    "expectedPolicyRevisionId": command.expected_policy_revision_id,
                }),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let binding = match PlatformRoleBinding::create(
                PlatformRoleBindingId::from_uuid(deterministic_id(
                    installation_id,
                    "platform-role-binding",
                    &idempotency,
                )),
                installation_id,
                command.principal_id,
                role,
                &policy,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let result = match repository
                .create_platform_role_binding(CreatePlatformRoleBindingWrite {
                    binding,
                    expected_policy_revision_id: command.expected_policy_revision_id,
                    actor_principal_id: command.actor_principal_id,
                    credential_id: command.credential_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(PlatformRoleBindingMutationResult {
                binding: result.value,
                replayed: result.replayed,
            }))
        })
    }
}

pub struct ChangePlatformRoleBindingHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn IPlatformRbacRepository>,
}

impl ChangePlatformRoleBindingHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn IPlatformRbacRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl CommandHandler<ChangePlatformRoleBinding> for ChangePlatformRoleBindingHandler {
    fn execute(
        &self,
        command: ChangePlatformRoleBinding,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<PlatformRoleBindingMutationResult>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let role = match PlatformRole::parse(&command.role) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected platform role binding version must be positive".into(),
                )));
            }
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let idempotency = match idempotency(
                format!(
                    "installation/platform-role-bindings/{}/role",
                    command.binding_id
                ),
                command.idempotency_key,
                &serde_json::json!({
                    "role": role.as_str(),
                    "expectedVersion": command.expected_version,
                    "expectedPolicyRevisionId": command.expected_policy_revision_id,
                }),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let result = match repository
                .change_platform_role_binding(ChangePlatformRoleBindingWrite {
                    installation_id,
                    binding_id: command.binding_id,
                    expected_version: command.expected_version,
                    expected_policy_revision_id: command.expected_policy_revision_id,
                    role,
                    actor_principal_id: command.actor_principal_id,
                    credential_id: command.credential_id,
                    changed_at: Utc::now(),
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(PlatformRoleBindingMutationResult {
                binding: result.value,
                replayed: result.replayed,
            }))
        })
    }
}

pub struct RevokePlatformRoleBindingHandler {
    bootstrap: Arc<dyn IIdentityBootstrapRepository>,
    repository: Arc<dyn IPlatformRbacRepository>,
}

impl RevokePlatformRoleBindingHandler {
    pub fn new(
        bootstrap: Arc<dyn IIdentityBootstrapRepository>,
        repository: Arc<dyn IPlatformRbacRepository>,
    ) -> Self {
        Self {
            bootstrap,
            repository,
        }
    }
}

impl CommandHandler<RevokePlatformRoleBinding> for RevokePlatformRoleBindingHandler {
    fn execute(
        &self,
        command: RevokePlatformRoleBinding,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<PlatformRoleBindingMutationResult>>,
    > {
        let bootstrap = Arc::clone(&self.bootstrap);
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "expected platform role binding version must be positive".into(),
                )));
            }
            let installation_id = match installation_id(&bootstrap).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let idempotency = match idempotency(
                format!(
                    "installation/platform-role-bindings/{}/revoke",
                    command.binding_id
                ),
                command.idempotency_key,
                &serde_json::json!({"expectedVersion": command.expected_version}),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let result = match repository
                .revoke_platform_role_binding(RevokePlatformRoleBindingWrite {
                    installation_id,
                    binding_id: command.binding_id,
                    expected_version: command.expected_version,
                    actor_principal_id: command.actor_principal_id,
                    credential_id: command.credential_id,
                    revoked_at: Utc::now(),
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(PlatformRoleBindingMutationResult {
                binding: result.value,
                replayed: result.replayed,
            }))
        })
    }
}

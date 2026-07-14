use crate::modules::identity::domain::value_objects::BootstrapCredential;
use a3s_boot::{BootError, BoxFuture, ExecutionContext, Guard, Result};

#[derive(Clone)]
pub struct BootstrapGuard {
    credential: BootstrapCredential,
}

impl BootstrapGuard {
    pub fn new(credential: BootstrapCredential) -> Self {
        Self { credential }
    }
}

impl Guard for BootstrapGuard {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        let credential = self.credential.clone();
        Box::pin(async move {
            let candidate = context
                .request
                .header("x-a3s-bootstrap-token")
                .ok_or_else(|| {
                    BootError::Unauthorized("missing bootstrap credential".to_string())
                })?;
            if !credential.verify(candidate) {
                return Err(BootError::Unauthorized(
                    "invalid bootstrap credential".to_string(),
                ));
            }
            Ok(true)
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OrganizationTenantGuard;

impl Guard for OrganizationTenantGuard {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async move {
            let Some(requested) = context.request.param("organization_id") else {
                return Ok(true);
            };
            let principal = context.request.require_auth_principal()?;
            if principal.has_role("platform_admin") {
                return Ok(true);
            }
            let authenticated = principal
                .claim("organization_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    BootError::Forbidden(
                        "authenticated principal has no organization context".to_string(),
                    )
                })?;
            if requested != authenticated {
                return Err(BootError::Forbidden(
                    "authenticated token cannot access another organization".to_string(),
                ));
            }
            Ok(true)
        })
    }
}

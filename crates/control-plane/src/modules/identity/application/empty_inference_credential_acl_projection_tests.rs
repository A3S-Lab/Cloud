//! Empty Identity credential ACL projection tests (I0.2b honesty brick).
//!
//! Fixture and credential-free managed paths intentionally wire Empty. These
//! tests certify Empty returns no credentials for any scopes rather than
//! inventing Identity facts.

use super::{
    EmptyInferenceCredentialAclProjectionPort, IInferenceCredentialAclProjectionPort,
    InferenceCredentialEnvironmentScope,
};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};

#[tokio::test]
async fn empty_port_returns_no_credentials_for_any_scopes() {
    let scopes = [
        InferenceCredentialEnvironmentScope::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
        )
        .expect("scope"),
        InferenceCredentialEnvironmentScope::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
        )
        .expect("scope"),
    ];
    let port = EmptyInferenceCredentialAclProjectionPort;
    let projections = port
        .list_inference_credential_acl_projections(&scopes)
        .await
        .expect("empty credential projection");
    assert!(
        projections.is_empty(),
        "Empty port must not invent inference credentials from environment scopes"
    );
}


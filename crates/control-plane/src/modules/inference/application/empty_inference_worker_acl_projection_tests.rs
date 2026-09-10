//! First-principles Empty worker ACL projection tests (I0.2b honesty brick).
//!
//! Power observation delivery is inactive; production must wire Empty and
//! Edge must not invent workers from route scopes. These tests certify that
//! intentional Empty composition, not a missing adapter.

use super::{
    postgres_inference_worker_acl_projections, EmptyInferenceWorkerAclProjectionPort,
    IInferenceWorkerAclProjectionPort, InferenceRouteEnvironmentScope,
};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use chrono::{Duration, Utc};

#[tokio::test]
async fn empty_port_returns_no_workers_for_any_scopes_or_projection_time() {
    let scopes = [
        InferenceRouteEnvironmentScope::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
        )
        .expect("scope"),
        InferenceRouteEnvironmentScope::new(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
        )
        .expect("scope"),
    ];
    let port = EmptyInferenceWorkerAclProjectionPort;
    let workers = port
        .list_inference_worker_acl_projections(&scopes, Utc::now() + Duration::hours(24))
        .await
        .expect("empty worker projection");
    assert!(
        workers.is_empty(),
        "Empty port must not invent workers from environment scopes"
    );
}

#[tokio::test]
async fn postgres_composition_factory_wires_empty_worker_port() {
    let port = postgres_inference_worker_acl_projections();
    let scope = InferenceRouteEnvironmentScope::new(
        OrganizationId::new(),
        ProjectId::new(),
        EnvironmentId::new(),
    )
    .expect("scope");
    let workers = port
        .list_inference_worker_acl_projections(std::slice::from_ref(&scope), Utc::now())
        .await
        .expect("postgres composition empty workers");
    assert!(
        workers.is_empty(),
        "Postgres composition must stay Empty until Power observation delivery"
    );
}

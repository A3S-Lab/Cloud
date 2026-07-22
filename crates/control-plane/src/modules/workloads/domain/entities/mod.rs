mod deployment;
mod secret_binding;
mod workload;
mod workload_revision;

pub use deployment::{Deployment, DeploymentStatus};
pub use secret_binding::{SecretBinding, SecretBindingTarget};
pub use workload::{Workload, WorkloadDesiredState};
pub use workload_revision::{
    ExternalBuildReference, HttpHealthCheck, OciArtifact, OciArtifactReference,
    RequestedServiceTemplate, ServicePort, ServiceProcess, ServiceResources, ServiceTemplate,
    WorkloadRevision,
};

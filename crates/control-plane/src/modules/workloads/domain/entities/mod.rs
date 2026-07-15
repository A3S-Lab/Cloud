mod deployment;
mod workload;
mod workload_revision;

pub use deployment::{Deployment, DeploymentStatus};
pub use workload::{Workload, WorkloadDesiredState};
pub use workload_revision::{
    HttpHealthCheck, OciArtifact, OciArtifactReference, RequestedServiceTemplate, ServicePort,
    ServiceProcess, ServiceResources, ServiceTemplate, WorkloadRevision,
};

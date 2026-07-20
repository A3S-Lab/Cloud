pub mod request;
pub mod response;

pub use request::{CreateWorkloadRequest, UpdateWorkloadRequest};
pub use response::{
    CancelDeploymentResponse, DeploymentResponse, WorkloadDeploymentResponse, WorkloadLogsResponse,
    WorkloadResponse, WorkloadStopResponse,
};

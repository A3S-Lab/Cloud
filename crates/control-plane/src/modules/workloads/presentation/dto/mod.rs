pub mod request;
pub mod response;

pub use request::CreateWorkloadRequest;
pub use response::{
    CancelDeploymentResponse, DeploymentResponse, WorkloadDeploymentResponse, WorkloadResponse,
    WorkloadStopResponse,
};

mod application;
mod application_release_contract;
mod events;
mod repository;
mod workflow_binding;

pub use application::{Application, ApplicationRelease};
pub use application_release_contract::{
    ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
    ApplicationInteractionMode, ApplicationReleaseContract, ApplicationReleaseContractSpec,
    ApplicationResponseMode, APPLICATION_RELEASE_CONTRACT_MAX_ACL_BYTES,
    APPLICATION_RELEASE_CONTRACT_SCHEMA,
};
pub use events::ApplicationReleasePublished;
pub(crate) use repository::ApplicationWriteReference;
pub use repository::{
    ApplicationRecord, CreateApplicationWrite, IApplicationRepository,
    PublishApplicationReleaseWrite,
};
pub use workflow_binding::{ApplicationWorkflowBinding, ApplicationWorkflowRevisionEvidence};

#[cfg(test)]
mod repository_tests;
#[cfg(test)]
mod tests;

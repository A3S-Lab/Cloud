mod application;
mod application_effect;
mod application_end_user;
mod application_invocation;
mod application_message;
mod application_release_contract;
mod application_session;
mod conversation_variables;
mod events;
mod repository;
mod session_repository;
mod workflow_binding;

pub use application::{Application, ApplicationRelease, APPLICATION_DESCRIPTION_MAX_CHARS};
pub use application_effect::ApplicationWorkflowEffect;
pub use application_end_user::ApplicationEndUser;
pub use application_invocation::{
    ApplicationInvocation, ApplicationInvocationStatus, APPLICATION_INVOCATION_INPUT_MAX_BYTES,
};
pub use application_message::{
    digest_json, ApplicationMessage, ApplicationMessageKind, APPLICATION_MESSAGE_MAX_BYTES,
};
pub use application_release_contract::{
    ApplicationAudience, ApplicationDeliveryPolicy, ApplicationExperience,
    ApplicationInteractionMode, ApplicationReleaseContract, ApplicationReleaseContractSpec,
    ApplicationResponseMode, APPLICATION_RELEASE_CONTRACT_MAX_ACL_BYTES,
    APPLICATION_RELEASE_CONTRACT_SCHEMA,
};
pub use application_session::{ApplicationSession, ApplicationSessionStatus};
pub use conversation_variables::{
    ConversationVariableRevision, APPLICATION_CONVERSATION_VARIABLES_MAX_BYTES,
};
pub use events::ApplicationReleasePublished;
pub(crate) use repository::ApplicationWriteReference;
pub use repository::{
    ApplicationRecord, CreateApplicationWrite, IApplicationRepository,
    PublishApplicationReleaseWrite,
};
pub use session_repository::{
    AdvanceApplicationInvocationWrite, AdvanceConversationVariablesWrite,
    AppendApplicationMessageWrite, CloseApplicationSessionWrite, IApplicationSessionRepository,
    OpenApplicationSessionWrite, RequestApplicationInvocationWrite,
};
pub use workflow_binding::{ApplicationWorkflowBinding, ApplicationWorkflowRevisionEvidence};

#[cfg(test)]
mod repository_tests;
#[cfg(test)]
mod session_repository_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod tests;

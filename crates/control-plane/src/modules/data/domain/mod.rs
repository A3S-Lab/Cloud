mod object_namespace;
mod object_namespace_credential;
mod object_namespace_provider_profile;
mod object_namespace_recovery;
mod object_namespace_retention;

pub use object_namespace::{
    IObjectNamespace, ObjectNamespaceError, ObjectNamespaceKey, ObjectNamespaceProbeEvidence,
    ObjectNamespaceRead, ObjectNamespaceVersion,
};
pub use object_namespace_credential::{
    ObjectNamespaceCredentialBinding, ObjectNamespaceCredentialBindingSpec,
};
pub use object_namespace_provider_profile::{
    ObjectNamespaceProviderProfile, ObjectNamespaceProviderProfileSpec,
    OBJECT_NAMESPACE_PROVIDER_PROFILE_MAX_ACL_BYTES, OBJECT_NAMESPACE_PROVIDER_PROFILE_SCHEMA,
};
pub use object_namespace_recovery::{
    ObjectNamespaceDeletionEvidence, ObjectNamespaceDeletionPlan, ObjectNamespaceDeletionPlanSpec,
    ObjectNamespaceRecoveryPoint, ObjectNamespaceRecoveryPointSpec, ObjectNamespaceRestoreEvidence,
    ObjectNamespaceRestorePlan, ObjectNamespaceRestorePlanSpec,
};
pub use object_namespace_retention::{
    ObjectNamespaceRetentionPolicy, ObjectNamespaceRetentionPolicySpec,
};

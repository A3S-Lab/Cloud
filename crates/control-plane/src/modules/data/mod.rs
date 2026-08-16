mod application;
mod domain;
mod infrastructure;

pub use application::{
    MaterializedObjectNamespaceCredentials, ObjectNamespaceConformanceProbe,
    ObjectNamespaceCredentialAdmission, ObjectNamespaceCredentialMaterializer,
};
pub use domain::{
    IObjectNamespace, ObjectNamespaceCredentialBinding, ObjectNamespaceCredentialBindingSpec,
    ObjectNamespaceDeletionEvidence, ObjectNamespaceDeletionPlan, ObjectNamespaceDeletionPlanSpec,
    ObjectNamespaceError, ObjectNamespaceKey, ObjectNamespaceProbeEvidence,
    ObjectNamespaceProviderProfile, ObjectNamespaceProviderProfileSpec, ObjectNamespaceRead,
    ObjectNamespaceRecoveryPoint, ObjectNamespaceRecoveryPointSpec, ObjectNamespaceRestoreEvidence,
    ObjectNamespaceRestorePlan, ObjectNamespaceRestorePlanSpec, ObjectNamespaceRetentionPolicy,
    ObjectNamespaceRetentionPolicySpec, ObjectNamespaceVersion,
    OBJECT_NAMESPACE_PROVIDER_PROFILE_MAX_ACL_BYTES, OBJECT_NAMESPACE_PROVIDER_PROFILE_SCHEMA,
};

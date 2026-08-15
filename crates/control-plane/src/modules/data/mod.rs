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
    ObjectNamespaceError, ObjectNamespaceKey, ObjectNamespaceProbeEvidence, ObjectNamespaceRead,
    ObjectNamespaceRecoveryPoint, ObjectNamespaceRecoveryPointSpec, ObjectNamespaceRestoreEvidence,
    ObjectNamespaceRestorePlan, ObjectNamespaceRestorePlanSpec, ObjectNamespaceRetentionPolicy,
    ObjectNamespaceRetentionPolicySpec, ObjectNamespaceVersion,
};

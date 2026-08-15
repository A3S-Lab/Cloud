mod object_namespace;
mod object_namespace_credential;

pub use object_namespace::{
    IObjectNamespace, ObjectNamespaceError, ObjectNamespaceKey, ObjectNamespaceProbeEvidence,
    ObjectNamespaceRead, ObjectNamespaceVersion,
};
pub use object_namespace_credential::{
    ObjectNamespaceCredentialBinding, ObjectNamespaceCredentialBindingSpec,
};

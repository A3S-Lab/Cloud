use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! identifier {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

identifier!(OrganizationId);
identifier!(ApiTokenId);
identifier!(ProjectId);
identifier!(EnvironmentId);
identifier!(OperationId);
identifier!(NodeId);
identifier!(EnrollmentTokenId);
identifier!(NodeCertificateId);
identifier!(NodeCommandId);
identifier!(WorkloadId);
identifier!(WorkloadRevisionId);
identifier!(DeploymentId);
identifier!(RouteId);
identifier!(DomainClaimId);
identifier!(GatewayCertificateId);
identifier!(SecretId);
identifier!(SourceRevisionId);

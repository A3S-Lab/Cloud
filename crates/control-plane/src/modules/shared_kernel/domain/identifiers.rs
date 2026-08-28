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
identifier!(InstallationId);
identifier!(PlatformRoleBindingId);
identifier!(PlatformRolePolicyId);
identifier!(PlatformRolePolicyRevisionId);
identifier!(PrincipalId);
identifier!(RecipientContactId);
identifier!(RecipientContactVerificationId);
identifier!(ExternalIdentityLinkId);
identifier!(OidcFlowId);
identifier!(MembershipId);
identifier!(MembershipInvitationId);
identifier!(ResourceGrantId);
identifier!(ApiTokenId);
identifier!(TrustDomainId);
identifier!(TrustDomainRevisionId);
identifier!(WorkloadIdentityPolicyId);
identifier!(WorkloadIdentityPolicyRevisionId);
identifier!(ProjectId);
identifier!(ProjectAttributionProfileId);
identifier!(NotificationId);
identifier!(NotificationAlertPolicyId);
identifier!(NotificationSubscriptionId);
identifier!(ApplicationEndUserId);
identifier!(ApplicationId);
identifier!(ApplicationInvocationId);
identifier!(ApplicationMessageId);
identifier!(ApplicationReleaseId);
identifier!(ApplicationSessionId);
identifier!(ConversationVariableRevisionId);
identifier!(UserFileId);
identifier!(UserFileUploadId);
identifier!(KnowledgeBaseId);
identifier!(KnowledgeBaseRevisionId);
identifier!(KnowledgeDocumentId);
identifier!(KnowledgeChunkId);
identifier!(KnowledgeIndexRevisionId);
identifier!(KnowledgeRetrievalPolicyRevisionId);
identifier!(ExternalKnowledgeBindingId);
identifier!(KnowledgePipelineId);
identifier!(KnowledgePipelineReleaseId);
identifier!(ConnectorProfileId);
identifier!(ConnectorRevisionId);
identifier!(DurableCellApplicationId);
identifier!(DurableCellApplicationRevisionId);
identifier!(StorageNamespaceId);
identifier!(EnvironmentId);
identifier!(PluginRegistryId);
identifier!(OperationId);
identifier!(NodeId);
identifier!(NodePoolId);
identifier!(EnrollmentTokenId);
identifier!(NodeCertificateId);
identifier!(NodeCommandId);
identifier!(WorkloadId);
identifier!(WorkloadRevisionId);
identifier!(WorkloadReplicaId);
identifier!(WorkloadReplicaMemberId);
identifier!(WorkloadPlacementGroupId);
identifier!(DeploymentId);
identifier!(ResourceClaimId);
identifier!(GatewayScopeId);
identifier!(GatewayRolloutId);
identifier!(RouteId);
identifier!(DomainClaimId);
identifier!(GatewayCertificateId);
identifier!(McpCredentialId);
identifier!(SecretId);
identifier!(SourceRevisionId);
identifier!(BuildPlanId);
identifier!(WorkloadProfileId);
identifier!(WorkloadProfileRevisionId);
identifier!(PullRequestPreviewId);
identifier!(PullRequestPreviewPolicyRevisionId);
identifier!(SourcePullRequestChangeId);
identifier!(SourceConnectionId);
identifier!(SourceSubscriptionId);
identifier!(BuildRunId);
identifier!(ExecutionId);
identifier!(ExecutionTemplateId);
identifier!(ExecutionTemplateRevisionId);
identifier!(AssetId);
identifier!(AssetReleaseId);
identifier!(AgentConversationId);
identifier!(AgentExecutionId);
identifier!(AgentExecutionCheckpointId);
identifier!(AgentApprovalCheckpointId);
identifier!(AgentApprovalDecisionId);
identifier!(OntologyId);
identifier!(OntologyRevisionId);
identifier!(WorkflowDefinitionId);
identifier!(WorkflowRevisionId);
identifier!(WorkflowGoalId);
identifier!(PlanRevisionId);
identifier!(WorkflowRunId);
identifier!(HumanTaskId);
identifier!(FormId);
identifier!(FormReleaseId);
identifier!(FormSubmissionId);
identifier!(WorkflowDecisionId);

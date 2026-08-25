use crate::modules::developer_workflows::domain::{
    CommitPullRequestPreviewProjection, IPullRequestPreviewProjectionRepository,
    PullRequestPreview, PullRequestPreviewProjectionReceipt, PullRequestPreviewVersion,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, IdempotentWrite, OrganizationId, ProjectId, RepositoryError,
    SourcePullRequestChangeId, SourceSubscriptionId,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

type ReceiptKey = (OrganizationId, SourcePullRequestChangeId);
type PreviewKey = (OrganizationId, SourceSubscriptionId, u64);

#[derive(Default)]
struct State {
    receipts: BTreeMap<ReceiptKey, PullRequestPreviewProjectionReceipt>,
    previews: BTreeMap<PreviewKey, PullRequestPreview>,
}

#[derive(Default)]
pub struct InMemoryPullRequestPreviewProjectionRepository {
    state: RwLock<State>,
}

impl InMemoryPullRequestPreviewProjectionRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl IPullRequestPreviewProjectionRepository for InMemoryPullRequestPreviewProjectionRepository {
    async fn find_receipt(
        &self,
        organization_id: OrganizationId,
        source_pull_request_change_id: SourcePullRequestChangeId,
    ) -> Result<Option<PullRequestPreviewProjectionReceipt>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .receipts
            .get(&(organization_id, source_pull_request_change_id))
            .cloned())
    }

    async fn find_preview(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        source_environment_id: EnvironmentId,
        source_subscription_id: SourceSubscriptionId,
        pull_request_id: u64,
    ) -> Result<Option<PullRequestPreview>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .previews
            .get(&(organization_id, source_subscription_id, pull_request_id))
            .filter(|preview| {
                preview.policy_authority.policy.project_id == project_id
                    && preview.policy_authority.source_environment_id == source_environment_id
            })
            .cloned())
    }

    async fn commit_projection(
        &self,
        write: CommitPullRequestPreviewProjection,
    ) -> Result<IdempotentWrite<PullRequestPreviewProjectionReceipt>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let mut state = self.state.write().await;
        let receipt_key = (
            write.receipt.organization_id,
            write.receipt.source_pull_request_change_id,
        );
        if let Some(existing) = state.receipts.get(&receipt_key) {
            if !same_fact(existing, &write.receipt) {
                return Err(RepositoryError::Conflict(
                    "Sources pull-request fact ID changed content or owner binding".into(),
                ));
            }
            return Ok(IdempotentWrite {
                value: existing.clone(),
                replayed: true,
            });
        }

        let preview_key = (
            write.receipt.organization_id,
            write.receipt.source_subscription_id,
            write.receipt.pull_request_id,
        );
        let observed = state
            .previews
            .get(&preview_key)
            .map(|preview| PullRequestPreviewVersion {
                id: preview.id,
                aggregate_version: preview.aggregate_version,
            });
        if observed != write.expected_preview {
            return Err(RepositoryError::Conflict(
                "pull-request Preview advanced before projection commit".into(),
            ));
        }
        if let Some(preview) = write.preview {
            state.previews.insert(preview_key, preview);
        }
        state.receipts.insert(receipt_key, write.receipt.clone());
        Ok(IdempotentWrite {
            value: write.receipt,
            replayed: false,
        })
    }
}

fn same_fact(
    existing: &PullRequestPreviewProjectionReceipt,
    candidate: &PullRequestPreviewProjectionReceipt,
) -> bool {
    existing.matches_fact(&candidate.fingerprint())
}

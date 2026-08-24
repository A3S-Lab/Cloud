mod git_reference;
mod github_account;
mod github_installation_id;
mod webhook_delivery_id;

pub use crate::modules::shared_kernel::domain::GitCommitSha;
// Transitional compatibility facade. The canonical physical ownership is the
// Sources published language; new cross-context consumers must use that path.
pub use crate::modules::sources::published::{
    BuildPlatform, BuildRecipe, GitProvider, GitRepository,
};
pub use git_reference::GitReference;
pub use github_account::{GithubAccountId, GithubAccountKind, GithubLogin};
pub use github_installation_id::GithubInstallationId;
pub use webhook_delivery_id::WebhookDeliveryId;

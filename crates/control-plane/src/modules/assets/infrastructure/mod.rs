mod git_repository;
pub mod persistence;

pub use git_repository::LocalAssetGitRepository;
pub use persistence::PostgresAssetRepository;

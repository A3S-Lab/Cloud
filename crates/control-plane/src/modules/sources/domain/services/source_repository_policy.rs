use crate::modules::sources::domain::{GitProvider, GitRepository};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRepositoryPolicy {
    allowed_identities: BTreeSet<String>,
    denied_identities: BTreeSet<String>,
}

impl SourceRepositoryPolicy {
    pub fn github(
        allowed_repositories: &[String],
        denied_repositories: &[String],
    ) -> Result<Self, String> {
        let allowed_identities = parse_identities(allowed_repositories)?;
        if allowed_identities.is_empty() {
            return Err("source repository allowlist must contain at least one repository".into());
        }
        Ok(Self {
            allowed_identities,
            denied_identities: parse_identities(denied_repositories)?,
        })
    }

    pub fn allows(&self, repository: &GitRepository) -> bool {
        !self.denied_identities.contains(repository.identity())
            && self.allowed_identities.contains(repository.identity())
    }

    pub fn require(&self, repository: &GitRepository) -> Result<(), String> {
        self.allows(repository)
            .then_some(())
            .ok_or_else(|| "source repository is not permitted by policy".into())
    }
}

fn parse_identities(repositories: &[String]) -> Result<BTreeSet<String>, String> {
    repositories
        .iter()
        .map(|repository| {
            GitRepository::parse(GitProvider::Github, repository)
                .map(|repository| repository.identity().to_owned())
        })
        .collect()
}

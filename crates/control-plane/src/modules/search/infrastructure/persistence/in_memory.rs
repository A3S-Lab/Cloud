use crate::modules::search::domain::{
    ISearchRepository, SearchQuery, SearchResult, SearchVisibility,
};
use crate::modules::shared_kernel::domain::{OrganizationId, RepositoryError};
use async_trait::async_trait;
use std::cmp::Ordering;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use tokio::sync::RwLock;

#[derive(Default)]
pub(crate) struct InMemorySearchRepository {
    projections: RwLock<Vec<SearchResult>>,
    query_count: AtomicUsize,
}

impl InMemorySearchRepository {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) async fn register(&self, projection: SearchResult) -> Result<(), RepositoryError> {
        projection.validate().map_err(RepositoryError::Storage)?;
        self.projections.write().await.push(projection);
        Ok(())
    }

    pub(crate) fn query_count(&self) -> usize {
        self.query_count.load(AtomicOrdering::Relaxed)
    }
}

#[async_trait]
impl ISearchRepository for InMemorySearchRepository {
    async fn search(
        &self,
        organization_id: OrganizationId,
        query: &SearchQuery,
        limit: u16,
        visibility: &SearchVisibility,
    ) -> Result<Vec<SearchResult>, RepositoryError> {
        self.query_count.fetch_add(1, AtomicOrdering::Relaxed);
        let query = query.as_str();
        let mut matches = self
            .projections
            .read()
            .await
            .iter()
            .filter(|projection| projection.organization_id == organization_id)
            .filter(|projection| projection.is_visible_to(visibility))
            .filter_map(|projection| {
                let title = projection.title.to_lowercase();
                let id = projection.id.to_string();
                let searchable = format!(
                    "{} {} {} {} {}",
                    projection.kind.as_str(),
                    title,
                    projection.description.to_lowercase(),
                    projection
                        .state
                        .as_deref()
                        .unwrap_or_default()
                        .to_lowercase(),
                    id
                );
                searchable
                    .contains(query)
                    .then_some((rank(&title, &id, query), projection.clone()))
            })
            .collect::<Vec<_>>();
        matches.sort_by(compare_matches);
        matches.truncate(usize::from(limit));
        Ok(matches
            .into_iter()
            .map(|(_, projection)| projection)
            .collect())
    }
}

fn rank(title: &str, id: &str, query: &str) -> u8 {
    if title == query || id == query {
        0
    } else if title.starts_with(query) {
        1
    } else if title.contains(query) {
        2
    } else {
        3
    }
}

fn compare_matches(
    (left_rank, left): &(u8, SearchResult),
    (right_rank, right): &(u8, SearchResult),
) -> Ordering {
    left_rank
        .cmp(right_rank)
        .then_with(|| left.kind.as_str().cmp(right.kind.as_str()))
        .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
        .then_with(|| left.id.cmp(&right.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::search::domain::{SearchResourceKind, SearchVisibilityScope};
    use crate::modules::shared_kernel::domain::ProjectId;
    use chrono::Utc;
    use uuid::Uuid;

    #[tokio::test]
    async fn ranks_exact_prefix_title_and_metadata_matches_deterministically() {
        let repository = InMemorySearchRepository::new();
        let organization_id = OrganizationId::new();
        for (kind, title, description) in [
            (SearchResourceKind::Node, "Cloud", "Node"),
            (SearchResourceKind::Node, "Cloud worker", "Node"),
            (SearchResourceKind::Node, "Worker cloud", "Node"),
            (SearchResourceKind::Operation, "Deploy", "cloud workflow"),
            (
                SearchResourceKind::PluginRegistry,
                "Registry",
                "cloud metadata",
            ),
            (SearchResourceKind::Operation, "Operation", "cloud metadata"),
        ] {
            repository
                .register(SearchResult {
                    organization_id,
                    project_id: None,
                    environment_id: None,
                    workload_id: None,
                    kind,
                    id: Uuid::new_v4(),
                    title: title.into(),
                    description: description.into(),
                    state: None,
                    updated_at: Utc::now(),
                })
                .await
                .expect("projection");
        }

        let results = repository
            .search(
                organization_id,
                &SearchQuery::parse("cloud").expect("query"),
                6,
                &SearchVisibility::organization_wide(),
            )
            .await
            .expect("search");

        assert_eq!(
            results
                .iter()
                .map(|result| result.title.as_str())
                .collect::<Vec<_>>(),
            [
                "Cloud",
                "Cloud worker",
                "Worker cloud",
                "Deploy",
                "Operation",
                "Registry",
            ]
        );
    }

    #[tokio::test]
    async fn authorization_is_applied_before_ranking_and_limit() {
        let repository = InMemorySearchRepository::new();
        let organization_id = OrganizationId::new();
        let denied_project_id = ProjectId::new();
        let allowed_project_id = ProjectId::new();
        for (project_id, title) in [
            (denied_project_id, "cloud"),
            (allowed_project_id, "cloud worker"),
        ] {
            repository
                .register(SearchResult {
                    organization_id,
                    project_id: Some(project_id.as_uuid()),
                    environment_id: None,
                    workload_id: None,
                    kind: SearchResourceKind::Project,
                    id: project_id.as_uuid(),
                    title: title.into(),
                    description: String::new(),
                    state: None,
                    updated_at: Utc::now(),
                })
                .await
                .expect("projection");
        }

        let visibility = SearchVisibility::restricted([SearchVisibilityScope::Project {
            project_id: allowed_project_id,
        }]);
        let results = repository
            .search(
                organization_id,
                &SearchQuery::parse("cloud").expect("query"),
                1,
                &visibility,
            )
            .await
            .expect("search");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, allowed_project_id.as_uuid());
    }
}

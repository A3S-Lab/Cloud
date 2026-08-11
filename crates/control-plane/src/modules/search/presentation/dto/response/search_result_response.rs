use crate::modules::search::domain::{SearchResourceKind, SearchResult};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultResponse {
    pub organization_id: Uuid,
    pub project_id: Option<Uuid>,
    pub environment_id: Option<Uuid>,
    pub workload_id: Option<Uuid>,
    pub kind: String,
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub state: Option<String>,
    pub href: String,
    pub updated_at: DateTime<Utc>,
}

impl From<SearchResult> for SearchResultResponse {
    fn from(result: SearchResult) -> Self {
        let href = search_href(&result);
        Self {
            organization_id: result.organization_id.as_uuid(),
            project_id: result.project_id,
            environment_id: result.environment_id,
            workload_id: result.workload_id,
            kind: result.kind.as_str().to_owned(),
            id: result.id,
            title: result.title,
            description: result.description,
            state: result.state,
            href,
            updated_at: result.updated_at,
        }
    }
}

fn search_href(result: &SearchResult) -> String {
    let organization = result.organization_id.as_uuid();
    let organization_root = format!("#/organizations/{organization}");
    match result.kind {
        SearchResourceKind::Project => {
            format!("{organization_root}/projects/{}", result.id)
        }
        SearchResourceKind::Ontology => result.project_id.map_or_else(
            || format!("{organization_root}/ontologies/{}", result.id),
            |project_id| {
                format!(
                    "{organization_root}/projects/{project_id}/ontologies/{}",
                    result.id
                )
            },
        ),
        SearchResourceKind::PluginRegistry => {
            format!("{organization_root}/plugin-registries/{}", result.id)
        }
        SearchResourceKind::Environment => {
            context_root(result).map_or(organization_root, |root| root)
        }
        SearchResourceKind::Node => format!("{organization_root}/nodes/{}", result.id),
        SearchResourceKind::Workload => contextual_resource(result, "workloads"),
        SearchResourceKind::Deployment => contextual_resource(result, "deployments"),
        SearchResourceKind::Route => contextual_resource(result, "routes"),
        SearchResourceKind::DomainClaim => contextual_resource(result, "domain-claims"),
        SearchResourceKind::GatewayScope => contextual_resource(result, "gateway-scopes"),
        SearchResourceKind::BuildRun => contextual_resource(result, "build-runs"),
        SearchResourceKind::SourceRevision => contextual_resource(result, "source-revisions"),
        SearchResourceKind::Secret => contextual_resource(result, "secrets"),
        SearchResourceKind::Operation => {
            format!("{organization_root}/operations/{}", result.id)
        }
    }
}

fn context_root(result: &SearchResult) -> Option<String> {
    Some(format!(
        "#/organizations/{}/projects/{}/environments/{}",
        result.organization_id.as_uuid(),
        result.project_id?,
        result.environment_id?
    ))
}

fn contextual_resource(result: &SearchResult, segment: &str) -> String {
    context_root(result).map_or_else(
        || {
            format!(
                "#/organizations/{}/{segment}/{}",
                result.organization_id.as_uuid(),
                result.id
            )
        },
        |root| format!("{root}/{segment}/{}", result.id),
    )
}

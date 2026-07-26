use crate::modules::shared_kernel::domain::OrganizationId;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SearchResourceKind {
    Project,
    Environment,
    Node,
    Workload,
    Deployment,
    Route,
    DomainClaim,
    GatewayScope,
    BuildRun,
    SourceRevision,
    Secret,
    Operation,
}

impl SearchResourceKind {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "project" => Ok(Self::Project),
            "environment" => Ok(Self::Environment),
            "node" => Ok(Self::Node),
            "workload" => Ok(Self::Workload),
            "deployment" => Ok(Self::Deployment),
            "route" => Ok(Self::Route),
            "domain_claim" => Ok(Self::DomainClaim),
            "gateway_scope" => Ok(Self::GatewayScope),
            "build_run" => Ok(Self::BuildRun),
            "source_revision" => Ok(Self::SourceRevision),
            "secret" => Ok(Self::Secret),
            "operation" => Ok(Self::Operation),
            _ => Err("search projection has an unknown resource kind".into()),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Environment => "environment",
            Self::Node => "node",
            Self::Workload => "workload",
            Self::Deployment => "deployment",
            Self::Route => "route",
            Self::DomainClaim => "domain_claim",
            Self::GatewayScope => "gateway_scope",
            Self::BuildRun => "build_run",
            Self::SourceRevision => "source_revision",
            Self::Secret => "secret",
            Self::Operation => "operation",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub organization_id: OrganizationId,
    pub project_id: Option<Uuid>,
    pub environment_id: Option<Uuid>,
    pub workload_id: Option<Uuid>,
    pub kind: SearchResourceKind,
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub state: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl SearchResult {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil() || self.id.is_nil() {
            return Err("search projection identifiers must not be nil".into());
        }
        validate_text("title", &self.title, 512, false)?;
        validate_text("description", &self.description, 512, true)?;
        if let Some(state) = &self.state {
            validate_text("state", state, 63, false)?;
        }
        match self.kind {
            SearchResourceKind::Project if self.project_id != Some(self.id) => {
                Err("project search projection must identify its project".into())
            }
            SearchResourceKind::Environment if self.environment_id != Some(self.id) => {
                Err("environment search projection must identify its environment".into())
            }
            SearchResourceKind::Workload
            | SearchResourceKind::Deployment
            | SearchResourceKind::Route
            | SearchResourceKind::DomainClaim
            | SearchResourceKind::GatewayScope
            | SearchResourceKind::BuildRun
            | SearchResourceKind::SourceRevision
            | SearchResourceKind::Secret
                if self.project_id.is_none() || self.environment_id.is_none() =>
            {
                Err("environment resource search projection is missing its context".into())
            }
            _ => Ok(()),
        }
    }
}

fn validate_text(
    label: &str,
    value: &str,
    maximum_chars: usize,
    allow_empty: bool,
) -> Result<(), String> {
    if (!allow_empty && value.is_empty())
        || value.chars().count() > maximum_chars
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!(
            "search projection {label} must contain at most {maximum_chars} safe characters"
        ));
    }
    Ok(())
}

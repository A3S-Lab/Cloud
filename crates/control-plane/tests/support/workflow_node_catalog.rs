use super::*;
use a3s_boot::{CqrsContext, ModuleRef, QueryHandler};
use a3s_cloud_control_plane::modules::identity::domain::services::ResourceAccessEvaluator;
use a3s_cloud_control_plane::modules::projects::domain::entities::Project;
use a3s_cloud_control_plane::modules::projects::domain::events::ProjectCreated;
use a3s_cloud_control_plane::modules::projects::domain::repositories::IProjectRepository;
use a3s_cloud_control_plane::modules::projects::domain::value_objects::ProjectName;
use a3s_cloud_control_plane::modules::projects::PostgresProjectsRepository;
use a3s_cloud_control_plane::modules::workflow::{
    GetWorkflowNodeCatalog, GetWorkflowNodeCatalogHandler, WorkflowNodeCatalog,
};

pub(super) async fn exercise_workflow_node_catalog_reconnect(
    url: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let (first, migration_count) = {
        let executor = connect_and_migrate(&url, 4).await?;
        let database = Database::new(PostgresDialect, executor.clone());
        let created_at = Utc::now();
        database
            .execute(
                sql_query::<()>(
                    "insert into organizations (id, name, name_key, aggregate_version, created_at) values (",
                )
                .bind(organization_id.as_uuid())
                .append(", 'Workflow node catalog tenant', ")
                .bind(format!("workflow-node-catalog-{organization_id}"))
                .append(", 1, ")
                .bind(created_at)
                .append(")"),
            )
            .await?;
        let projects = Arc::new(PostgresProjectsRepository::new(executor));
        let project = Project::create(
            organization_id,
            project_id,
            ProjectName::parse("Workflow node catalog")?,
            created_at,
        );
        projects
            .create(
                project.clone(),
                ProjectCreated::envelope(&project, Uuid::now_v7())?,
                IdempotencyRequest::new(
                    format!("organizations/{organization_id}/projects"),
                    "postgres:workflow-node-catalog:create-project",
                    b"workflow-node-catalog",
                )?,
            )
            .await?;
        let migration_count = database
            .fetch_one_as(sql_query::<i64>("select count(*) from a3s_orm_migrations"))
            .await?;
        let catalog_table_count = database
            .fetch_one_as(sql_query::<i64>(
                "select count(*) from information_schema.tables where table_schema = 'public' and table_name like 'workflow_node_catalog%'",
            ))
            .await?;
        assert_eq!(catalog_table_count, 0);
        (
            query_catalog(projects, organization_id, project_id).await?,
            migration_count,
        )
    };

    let executor = connect_and_migrate(&url, 4).await?;
    let database = Database::new(PostgresDialect, executor.clone());
    assert_eq!(
        database
            .fetch_one_as(sql_query::<i64>("select count(*) from a3s_orm_migrations"))
            .await?,
        migration_count
    );
    let projects = Arc::new(PostgresProjectsRepository::new(executor));
    let reconnected = query_catalog(projects, organization_id, project_id).await?;
    assert_eq!(reconnected, first);
    assert_eq!(reconnected.nodes.len(), 23);
    assert!(!reconnected.parity_claim);
    Ok(())
}

async fn query_catalog(
    projects: Arc<PostgresProjectsRepository>,
    organization_id: OrganizationId,
    project_id: ProjectId,
) -> Result<WorkflowNodeCatalog, Box<dyn std::error::Error>> {
    let projects: Arc<dyn IProjectRepository> = projects;
    Ok(GetWorkflowNodeCatalogHandler::new(projects)
        .execute(
            GetWorkflowNodeCatalog {
                organization_id,
                project_id,
                resource_access: ResourceAccessEvaluator::organization_wide(),
            },
            CqrsContext::new(ModuleRef::new()),
        )
        .await??)
}

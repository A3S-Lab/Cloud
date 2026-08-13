pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::commands::create_environment::{
    CreateEnvironment, CreateEnvironmentHandler, CreateEnvironmentResult,
};
pub use application::commands::create_project::{
    CreateProject, CreateProjectHandler, CreateProjectResult,
};
pub use application::commands::update_project_attribution::{
    UpdateProjectAttribution, UpdateProjectAttributionHandler, UpdateProjectAttributionResult,
};
pub use application::queries::get_project_attribution::{
    GetProjectAttribution, GetProjectAttributionHandler,
};
pub use application::queries::list_environments::{ListEnvironments, ListEnvironmentsHandler};
pub use application::queries::list_projects::{ListProjects, ListProjectsHandler};
pub use infrastructure::persistence::{InMemoryProjectsRepository, PostgresProjectsRepository};
pub use presentation::ProjectsModule;

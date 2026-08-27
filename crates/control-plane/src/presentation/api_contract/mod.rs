mod agent_components;
mod components;
mod developer_workflow_components;
mod developer_workflow_documentation;
mod developer_workflow_operation;
mod developer_workflow_route;
mod document;
mod documentation;
mod documentation_examples;
mod documentation_tags;
mod operation;
mod request_schema;
mod route;
mod source_components;
mod workflow_components;
mod workflow_goal_components;
mod workflow_human_task_components;
mod workflow_ontology_components;
mod workflow_run_components;
mod workflow_run_observation_components;
mod workload_profile_components;
mod workload_profile_documentation;
mod workload_profile_operation;

pub use document::generate_openapi_contract;
pub use route::{openapi_info, ApiContractModule};

pub const API_PREFIX: &str = "/api/v1";
pub const API_MAJOR_VERSION: u16 = 1;
pub const OPENAPI_CONTRACT_VERSION: &str = "1.74.0";
pub const OPENAPI_DOCUMENT_PATH: &str = "/openapi.json";
pub const OPENAPI_PUBLIC_PATH: &str = "/api/v1/openapi.json";
pub const API_CONTRACT_VERSION_HEADER: &str = "x-a3s-api-contract-version";
pub const MINIMUM_DEPRECATION_DAYS: u16 = 180;

const HTTP_METHODS: [&str; 7] = ["delete", "get", "head", "options", "patch", "post", "put"];

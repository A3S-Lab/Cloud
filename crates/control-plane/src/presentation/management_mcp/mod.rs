mod arguments;
mod artifacts;
mod audit;
mod catalog;
mod dispatch;
mod edge;
mod execution_templates;
mod forms;
mod handler;
mod identity;
mod module;
mod nodes;
mod ontology;
mod operations;
mod plugins;
mod projects;
mod protocol;
mod search;
mod tool_result;
mod workflow;
mod workloads;

pub use module::ManagementMcpModule;

pub use a3s_cloud_contracts::MCP_PROTOCOL_VERSION as MANAGEMENT_MCP_PROTOCOL_VERSION;

mod arguments;
mod artifacts;
mod catalog;
mod dispatch;
mod edge;
mod handler;
mod module;
mod nodes;
mod operations;
mod projects;
mod protocol;
mod search;
mod tool_result;
mod workloads;

pub use module::ManagementMcpModule;

pub const MANAGEMENT_MCP_PROTOCOL_VERSION: &str = "2025-06-18";

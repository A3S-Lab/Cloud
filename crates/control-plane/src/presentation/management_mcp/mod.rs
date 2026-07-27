mod catalog;
mod handler;
mod module;
mod projects;
mod protocol;
mod search;
mod tool_result;

pub use module::ManagementMcpModule;

pub const MANAGEMENT_MCP_PROTOCOL_VERSION: &str = "2025-06-18";

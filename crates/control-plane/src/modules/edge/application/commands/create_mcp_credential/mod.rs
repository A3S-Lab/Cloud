mod command;
mod handler;

pub use command::CreateMcpCredential;
pub use handler::CreateMcpCredentialHandler;
pub(in crate::modules::edge::application::commands) use handler::{
    identity_collision, issuance_error,
};

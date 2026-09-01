//! A3S Cloud modular control plane.

mod access_projection;
pub mod app;
pub mod config;
#[cfg(feature = "persistence-conformance")]
#[doc(hidden)]
pub mod conformance;
pub mod infrastructure;
pub mod modules;
pub mod presentation;
mod server;

pub use app::{build_application, ControlPlaneStartupError};
pub use config::{CloudConfig, ProcessRole};
pub use server::ControlPlane;

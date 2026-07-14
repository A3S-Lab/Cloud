//! A3S Cloud modular control plane.

pub mod app;
pub mod config;
pub mod infrastructure;
pub mod modules;
pub mod presentation;
mod server;

pub use app::{build_application, ControlPlaneStartupError};
pub use config::{CloudConfig, ProcessRole};
pub use server::ControlPlane;

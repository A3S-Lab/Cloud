pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::commands::rebuild_operation_projections::{
    RebuildOperationProjectionsError, RebuildOperationProjectionsHandler,
    RebuildOperationProjectionsReport,
};
pub use application::commands::reconcile_operations::{
    OperationReconcileFailure, ReconcileOperationsHandler, ReconcileOperationsReport,
};
pub use application::queries::list_operations::{ListOperations, ListOperationsHandler};
pub use application::OperationReconciler;
pub use domain::entities::{
    OperationProjection, OperationRecord, OperationRequest, OperationStatus,
};
pub use domain::repositories::IOperationRepository;
pub use domain::services::{IOperationEngine, OperationEngineError};
pub use domain::value_objects::{OperationSubject, WorkflowIdentity};
pub use infrastructure::persistence::{InMemoryOperationRepository, PostgresOperationRepository};
pub use infrastructure::FlowOperationEngine;
pub use presentation::OperationsModule;

#[cfg(test)]
mod tests;

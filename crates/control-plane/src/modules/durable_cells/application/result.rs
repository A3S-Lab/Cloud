use crate::modules::durable_cells::domain::DurableCellApplicationRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCellApplicationMutationResult {
    pub record: DurableCellApplicationRecord,
    pub replayed: bool,
}

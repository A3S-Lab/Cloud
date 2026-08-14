use crate::modules::connectors::domain::ConnectorRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorProfileMutationResult {
    pub record: ConnectorRecord,
    pub replayed: bool,
}

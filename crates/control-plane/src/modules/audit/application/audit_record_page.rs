use crate::modules::audit::domain::{
    AuditRecordCursor, AuditRecordFilter, AuditRecordPage, IAuditRecordRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::OrganizationId;

use super::MAXIMUM_AUDIT_RECORD_LIMIT;

pub(super) async fn query_audit_record_page(
    repository: &dyn IAuditRecordRepository,
    organization_id: OrganizationId,
    filter: &AuditRecordFilter,
    cursor: Option<&str>,
    limit: usize,
) -> ApplicationResult<AuditRecordPage> {
    if limit == 0 || limit > MAXIMUM_AUDIT_RECORD_LIMIT {
        return Err(ApplicationError::Invalid(format!(
            "audit record limit must be between 1 and {MAXIMUM_AUDIT_RECORD_LIMIT}"
        )));
    }
    filter.validate().map_err(ApplicationError::Invalid)?;
    let cursor = cursor
        .map(AuditRecordCursor::parse)
        .transpose()
        .map_err(ApplicationError::Invalid)?;
    let mut records = repository
        .list_page(organization_id, filter, cursor, limit + 1)
        .await?;
    let next_cursor =
        (records.len() > limit).then(|| AuditRecordCursor::after(&records[limit - 1]).encode());
    records.truncate(limit);
    Ok(AuditRecordPage {
        records,
        next_cursor,
    })
}

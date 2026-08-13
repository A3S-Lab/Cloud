mod in_memory;
mod postgres;

pub use in_memory::InMemoryAuditRecordRepository;
pub use postgres::PostgresAuditRecordRepository;

#[cfg(test)]
mod typed_orm_tests;

mod in_memory;
mod postgres;

pub use in_memory::InMemoryWorkloadRepository;
pub use postgres::PostgresWorkloadRepository;

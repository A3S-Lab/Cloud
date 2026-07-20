mod in_memory;
mod postgres;

pub use in_memory::InMemorySourceRevisionRepository;
pub use postgres::PostgresSourceRevisionRepository;

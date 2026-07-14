mod in_memory;
mod postgres;

pub use in_memory::InMemoryProjectsRepository;
pub use postgres::PostgresProjectsRepository;

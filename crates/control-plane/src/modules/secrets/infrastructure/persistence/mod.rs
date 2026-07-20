mod in_memory;
mod postgres;

pub use in_memory::InMemorySecretRepository;
pub use postgres::PostgresSecretRepository;

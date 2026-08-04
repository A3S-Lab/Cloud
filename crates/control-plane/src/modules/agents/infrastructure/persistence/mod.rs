mod in_memory;
mod postgres;

pub use in_memory::InMemoryAgentRepository;
pub use postgres::PostgresAgentRepository;

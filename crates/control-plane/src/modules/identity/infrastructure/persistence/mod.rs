mod in_memory;
mod postgres;

pub use in_memory::InMemoryIdentityRepository;
pub use postgres::PostgresIdentityRepository;

mod in_memory;
mod postgres;

pub use in_memory::InMemoryBuildRunRepository;
pub use postgres::PostgresBuildRunRepository;

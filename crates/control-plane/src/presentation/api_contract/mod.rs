mod components;
mod document;
mod documentation;
mod documentation_examples;
mod documentation_tags;
mod operation;
mod request_schema;
mod route;

pub use document::generate_openapi_contract;
pub use route::{openapi_info, ApiContractModule};

pub const API_PREFIX: &str = "/api/v1";
pub const API_MAJOR_VERSION: u16 = 1;
pub const OPENAPI_CONTRACT_VERSION: &str = "1.60.0";
pub const OPENAPI_DOCUMENT_PATH: &str = "/openapi.json";
pub const OPENAPI_PUBLIC_PATH: &str = "/api/v1/openapi.json";
pub const API_CONTRACT_VERSION_HEADER: &str = "x-a3s-api-contract-version";
pub const MINIMUM_DEPRECATION_DAYS: u16 = 180;

const HTTP_METHODS: [&str; 7] = ["delete", "get", "head", "options", "patch", "post", "put"];

mod command;
mod handler;

pub use command::{
    DeactivateGithubRepositorySubscription, DeactivateGithubRepositorySubscriptionResult,
};
pub use handler::DeactivateGithubRepositorySubscriptionHandler;

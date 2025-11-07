//! Actors module root: exports submodules and convenient re-exports.

pub mod types;
pub mod actor_router;
pub mod account;
pub mod leader_card;
pub mod subscriber;

// Re-exports so you can `use crate::actors::Account` etc.
pub use types::*;
pub use actor_router::ActorRouter;
pub use account::Account;
pub use leader_card::LeaderCard;
pub use subscriber::Subscriber;

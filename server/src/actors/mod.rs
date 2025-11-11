//! Root module for all Actix actors used by a distributed YPF Ruta node.
//!
//! This module organizes actors that implement the node's local application
//! logic. Each actor encapsulates a unit of concurrency and state, following
//! the Actix actor model.
//!
//! Typical hierarchy:
//! ```text
//! ActorRouter (node root)
//!  └── AccountActor (one per account)
//!       └── CardActor (one per card)
//! ```
//!
//! Responsibilities:
//! - ActorRouter: entry point for routing messages between network and local actors.
//! - AccountActor: manages account-level state, limits and aggregation for its cards.
//! - CardActor: manages a single card's record, TTL, and local updates.
//!
//! Public types and actors are re-exported here for convenient imports.

pub mod types;
pub mod actor_router;
pub mod account;
pub mod card;

// Re-exports to simplify imports from other modules:
// Example: `use crate::actors::ActorRouter;`
pub use types::*;
pub use actor_router::ActorRouter;
pub use account::AccountActor;
pub use card::CardActor;

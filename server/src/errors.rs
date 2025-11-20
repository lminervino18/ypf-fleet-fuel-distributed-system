//! Application error types for the YPF Ruta server.
//!
//! This module defines a single application-level error enum (`AppError`),
//! a convenience result alias (`AppResult<T>`), and domain-specific error
//! enums used across the system (limit checks, verify/apply).
//!
//! Prefer returning `AppError` from higher-level functions and use `#[from]`
//! conversions where appropriate to map lower-level errors (IO, parsing,
//! serialization) into `AppError`.

use std::io;
use std::net::AddrParseError;
use thiserror::Error;
use tokio::task::JoinError;

/// Unified application-level error type.
///
/// Variants are grouped by concern (configuration, networking, actor/messaging,
/// protocol/serialization, fallback/unexpected).
#[derive(Error, Debug)]
pub enum AppError {
    // ---- Config / CLI ----
    #[error("Invalid configuration: {0}")]
    Config(String),

    #[error("Invalid coordinates: lat={lat}, lon={lon}")]
    InvalidCoords { lat: f64, lon: f64 },

    #[error("Invalid neighbor definition: {details}")]
    InvalidNeighbor { details: String },

    // ---- Networking ----
    #[error("Network I/O error: {source}")]
    Network {
        #[from]
        source: io::Error,
    },

    #[error("Failed to parse socket address: {source}")]
    AddrParse {
        #[from]
        source: AddrParseError,
    },

    #[error("Connection refused to {addr}")]
    ConnectionRefused { addr: String },

    #[error("Connection timed out to {addr}")]
    ConnectionTimeout { addr: String },

    #[error("Connection I/O error with {addr}: {source}")]
    ConnectionIO { addr: String, source: io::Error },

    #[error("Connection limit reached: max = {max}")]
    ConnectionLimit { max: usize },

    // ---- Actor / Messaging ----
    #[error("Actor not found: {id}")]
    ActorNotFound { id: String },

    #[error("Actix system error: {details}")]
    ActorSystem { details: String },

    #[error("Channel communication error: {details}")]
    Channel { details: String },

    #[error("Task join error: {0}")]
    Join(#[from] JoinError),

    // ---- Serialization / Protocol ----
    #[error("Serialization failed: {source}")]
    Serialization {
        #[from]
        source: serde_json::Error,
    },

    #[error("Invalid or malformed data: {details}")]
    InvalidData { details: String },

    #[error("Protocol parse error: {details}")]
    ProtocolParse { details: String },

    #[error("Invalid protocol format: {details}")]
    InvalidProtocol { details: String },

    #[error("Unknown message type: {details}")]
    UnknownMessage { details: String },

    #[error("Empty message: {details}")]
    EmptyMessage { details: String },

    // ---- Domain / Business logic ----
    /// Verification-phase error (Verify(op)) at application level.
    ///
    /// You can use this variant when you want to bubble up verify failures
    /// as `AppError` instead of keeping them local to the actors.
    #[error("Verification failed: {0}")]
    Verify(#[from] VerifyError),

    /// Application-phase error (Apply(op)) at application level.
    #[error("Apply failed: {0}")]
    Apply(#[from] ApplyError),

    // ---- Fallback / Unexpected ----
    #[error("Unexpected error: {details}")]
    Unexpected { details: String },
}

/// Standard application-wide Result type.
///
/// Use `AppResult<T>` as the return type for functions that may fail with
/// `AppError`. This keeps signatures concise and consistent across modules.
pub type AppResult<T> = Result<T, AppError>;


// ======================================================================
// Domain / business-logic error types (centralized here)
// ======================================================================

/// Error produced when checking limits for a `Charge` operation.
///
/// This is used during the Verify phase when evaluating card/account limits.
#[derive(Error, Debug, Clone)]
pub enum LimitCheckError {
    #[error("Card limit exceeded")]
    CardLimitExceeded,

    #[error("Account limit exceeded")]
    AccountLimitExceeded,
}

/// Error produced when attempting to update a limit (for `LimitAccount` / `LimitCard`).
///
/// This is used both in the Verify phase (to decide if a limit change is allowed)
/// and in the Apply phase (if applying the change fails).
#[derive(Error, Debug, Clone)]
pub enum LimitUpdateError {
    /// Attempted to set a limit below the already consumed amount.
    #[error("New limit is below current usage")]
    BelowCurrentUsage,
}

/// Verification-phase error (Verify(op)).
///
/// This is what the actor layer uses internally and exposes in `ActorEvent::VerifyResult`.
#[derive(Error, Debug, Clone)]
pub enum VerifyError {
    /// Failure due to charge limit checks.
    #[error("Charge limit check failed: {0}")]
    ChargeLimit(#[from] LimitCheckError),

    /// Failure when validating a limit change.
    #[error("Limit update validation failed: {0}")]
    LimitUpdate(#[from] LimitUpdateError),
}

/// Application-phase error (Apply(op)).
///
/// This is what the actor layer uses internally and exposes in
/// `ActorEvent::ApplyResult`.
#[derive(Error, Debug, Clone)]
pub enum ApplyError {
    /// Failure while applying a limit change.
    #[error("Limit update apply failed: {0}")]
    LimitUpdate(#[from] LimitUpdateError),
}

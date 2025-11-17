//! Application error types for the YPF Ruta server.
//!
//! This module defines a single application-level error enum (`AppError`) and
//! a convenience result alias (`AppResult<T>`). Prefer returning `AppError`
//! from higher-level functions and use `#[from]` conversions where appropriate
//! to map lower-level errors (IO, parsing, serialization) into `AppError`.
//!
//! The enum groups common error categories (configuration, networking,
//! actor/messaging, serialization/protocol, and unexpected failures) to
//! simplify error handling and logging across the codebase.

use std::io;
use std::net::AddrParseError;
use thiserror::Error;
use tokio::task::JoinError;

/// Unified application-level error type.
///
/// Variants are grouped by concern (configuration, networking, actor/messaging,
/// protocol/serialization, fallback). Many variants use `#[from]` to allow
/// ergonomic `?` propagation from underlying error types.
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

    // ---- Fallback / Unexpected ----
    #[error("Unexpected error: {details}")]
    Unexpected { details: String },
}

/// Standard application-wide Result type.
///
/// Use `AppResult<T>` as the return type for functions that may fail with
/// `AppError`. This keeps signatures concise and consistent across modules.
pub type AppResult<T> = Result<T, AppError>;

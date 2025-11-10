use thiserror::Error;
use std::io;
use std::net::AddrParseError;
use tokio::task::JoinError;

/// Unified application-level error type.
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
    ConnectionIO {
        addr: String,
        source: io::Error,
    },

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

    // ---- Fallback / Unexpected ----
    #[error("Unexpected error: {details}")]
    Unexpected { details: String },
}

/// Standard application-wide Result type.
pub type AppResult<T> = Result<T, AppError>;

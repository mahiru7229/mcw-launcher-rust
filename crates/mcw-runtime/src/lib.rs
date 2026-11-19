pub mod java;
pub mod launcher;

pub use java::{JavaDiscovery, JavaInstallation, JavaMajorPolicy, JavaResolver};
pub use launcher::{LaunchSpec, ProcessLauncher};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("Invalid Java major version: {0}")]
    InvalidJavaMajor(String),
    #[error("Java {0} is not supported (supported majors: 8, 17, 21, 25)")]
    UnsupportedJavaMajor(u32),
    #[error("No compatible Java runtime found (required Java {required}+). Install Java {managed} or pass --java <path>.")]
    NoCompatibleJava { required: u32, managed: u32 },
    #[error("Configured Java executable is invalid at {path}: {reason}")]
    InvalidJavaPath { path: String, reason: String },
    #[error("Failed to spawn Minecraft process: {0}")]
    SpawnFailed(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, RuntimeError>;

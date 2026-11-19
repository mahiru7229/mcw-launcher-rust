pub mod bytecode;
pub mod java;
pub mod launcher;
pub mod provisioner;
pub mod supervisor;

pub use bytecode::{detect_jar_required_java, detect_mods_required_java};
pub use java::{JavaDiscovery, JavaInstallation, JavaMajorPolicy, JavaResolver};
pub use launcher::{LaunchSpec, ProcessLauncher};
pub use provisioner::{AdoptiumRelease, JavaProvisioner};
pub use supervisor::{
    detect_unsupported_class_version, ExitClassification, LogLine, LogSink, LogStream, NoopLogSink,
    ProcessControl, ProcessExitReport, SupervisedProcess,
};

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
    #[error("Managed Java provisioning failed: {0}")]
    ProvisionFailed(String),
    #[error("Failed to spawn Minecraft process: {0}")]
    SpawnFailed(#[from] std::io::Error),
    #[error("Network error: {0}")]
    Net(#[from] mcw_net::NetError),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

pub type Result<T> = std::result::Result<T, RuntimeError>;

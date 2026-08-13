//! Framework-neutral target manifests, scenarios, and HTTP/webhook drivers.

pub mod driver;
pub mod manifest;
pub mod scenario;

pub use driver::{
    DefaultDriverRegistry, DriverError, EnvironmentSecretResolver, HttpTextDriver, SecretResolver,
    TargetDriver, TargetDriverRegistry, WebhookDriver,
};
pub use manifest::{
    CapabilityReport, DriverType, EvidenceMode, ManifestError, RegisteredTarget, TargetEnvironment,
    TargetManifest, validate_manifest, validate_registration,
};
pub use scenario::{
    RunContext, ScenarioDefinition, ScenarioError, TargetOutput, TargetResponseEnvelope,
    TargetSession, TestEvent, validate_scenario,
};

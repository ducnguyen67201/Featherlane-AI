use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use super::{
    DriverError, SecretResolver, TargetDriver, reset_common, send_json, start_session_common,
    validate_common,
};
use crate::{CapabilityReport, RunContext, TargetManifest, TargetOutput, TargetSession, TestEvent};

#[derive(Clone, Debug)]
pub struct HttpTextDriver {
    resolver: Arc<dyn SecretResolver>,
}

impl HttpTextDriver {
    pub fn new(resolver: Arc<dyn SecretResolver>) -> Self {
        Self { resolver }
    }
}

#[async_trait]
impl TargetDriver for HttpTextDriver {
    async fn validate(&self, manifest: &TargetManifest) -> Result<CapabilityReport, DriverError> {
        validate_common(self.resolver.as_ref(), manifest).await
    }

    async fn start_session(&self, context: RunContext) -> Result<TargetSession, DriverError> {
        start_session_common(context).await
    }

    async fn reset(
        &self,
        manifest: &TargetManifest,
        session: &TargetSession,
    ) -> Result<(), DriverError> {
        reset_common(self.resolver.as_ref(), manifest, session).await
    }

    async fn send(
        &self,
        manifest: &TargetManifest,
        session: &TargetSession,
        event: &TestEvent,
    ) -> Result<TargetOutput, DriverError> {
        let TestEvent::UserText { text } = event else {
            return Err(DriverError::UnsupportedEvent);
        };
        send_json(
            self.resolver.as_ref(),
            manifest,
            session,
            &json!({"session_id": session.id, "message": text}),
        )
        .await
    }
}

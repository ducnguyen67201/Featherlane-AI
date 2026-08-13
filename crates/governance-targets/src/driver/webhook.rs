use std::sync::Arc;

use async_trait::async_trait;

use super::{
    DriverError, SecretResolver, TargetDriver, reset_common, send_json, start_session_common,
    validate_common,
};
use crate::{CapabilityReport, RunContext, TargetManifest, TargetOutput, TargetSession, TestEvent};

#[derive(Clone, Debug)]
pub struct WebhookDriver {
    resolver: Arc<dyn SecretResolver>,
}

impl WebhookDriver {
    pub fn new(resolver: Arc<dyn SecretResolver>) -> Self {
        Self { resolver }
    }
}

#[async_trait]
impl TargetDriver for WebhookDriver {
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
        let (TestEvent::Webhook { payload } | TestEvent::System { payload }) = event else {
            return Err(DriverError::UnsupportedEvent);
        };
        send_json(self.resolver.as_ref(), manifest, session, payload).await
    }
}

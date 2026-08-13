use async_trait::async_trait;
use governance_application::{ApplicationError, SourceArtifactStore};
use governance_config::PolicyImportConfig;
use opendal::{Operator, services};
use std::fmt;

#[derive(Clone)]
pub struct OpenDalArtifactStore {
    operator: Operator,
}

impl fmt::Debug for OpenDalArtifactStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenDalArtifactStore")
            .finish_non_exhaustive()
    }
}

impl OpenDalArtifactStore {
    /// Builds an S3-compatible artifact store from typed configuration.
    ///
    /// # Errors
    ///
    /// Returns an error when the storage operator cannot be constructed.
    pub fn from_config(config: &PolicyImportConfig) -> Result<Self, ApplicationError> {
        if config.object_store_access_key_id.trim().is_empty()
            || config.object_store_secret_access_key.trim().is_empty()
        {
            return Err(ApplicationError::Unavailable(
                "object-store credentials are not configured".to_owned(),
            ));
        }
        let builder = services::S3::default()
            .bucket(&config.object_store_bucket)
            .region(&config.object_store_region)
            .endpoint(&config.object_store_url)
            .access_key_id(&config.object_store_access_key_id)
            .secret_access_key(&config.object_store_secret_access_key);
        let operator = Operator::new(builder)
            .map_err(|_| {
                ApplicationError::Unavailable("object-store configuration is invalid".to_owned())
            })?
            .finish();
        Ok(Self { operator })
    }
}

#[async_trait]
impl SourceArtifactStore for OpenDalArtifactStore {
    async fn put(&self, key: &str, content: Vec<u8>) -> Result<(), ApplicationError> {
        self.operator
            .write(key, content)
            .await
            .map_err(|_| ApplicationError::Unavailable("object-store write failed".to_owned()))?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, ApplicationError> {
        self.operator
            .read(key)
            .await
            .map(|buffer| buffer.to_vec())
            .map_err(|_| ApplicationError::Unavailable("object-store read failed".to_owned()))
    }
}

#[derive(Clone, Debug)]
pub struct MemoryArtifactStore {
    operator: Operator,
}

impl MemoryArtifactStore {
    /// Creates an isolated in-memory artifact store for tests and explicit local use.
    ///
    /// # Errors
    ///
    /// Returns an error when the in-memory operator cannot be constructed.
    pub fn new() -> Result<Self, ApplicationError> {
        let operator = Operator::new(services::Memory::default())
            .map_err(|_| {
                ApplicationError::Unavailable("memory object store is invalid".to_owned())
            })?
            .finish();
        Ok(Self { operator })
    }
}

#[async_trait]
impl SourceArtifactStore for MemoryArtifactStore {
    async fn put(&self, key: &str, content: Vec<u8>) -> Result<(), ApplicationError> {
        self.operator
            .write(key, content)
            .await
            .map_err(|_| ApplicationError::Unavailable("memory object write failed".to_owned()))?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, ApplicationError> {
        self.operator
            .read(key)
            .await
            .map(|buffer| buffer.to_vec())
            .map_err(|_| ApplicationError::Unavailable("memory object read failed".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_store_round_trips_bytes() {
        let store = MemoryArtifactStore::new().expect("memory store should build");
        store
            .put("org/import/raw", b"policy".to_vec())
            .await
            .expect("write should work");
        assert_eq!(
            store.get("org/import/raw").await.expect("read should work"),
            b"policy"
        );
    }
}

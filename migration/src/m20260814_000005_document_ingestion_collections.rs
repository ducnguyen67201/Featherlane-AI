use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[allow(clippy::too_many_lines)]
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r"
ALTER TABLE policy_imports
  ADD COLUMN processing_object_key TEXT,
  ADD COLUMN processing_content_sha256 VARCHAR(255),
  ADD COLUMN processing_mime_type VARCHAR(255),
  ADD COLUMN active_transformation_id UUID,
  ADD COLUMN ingestion_item_id UUID,
  ADD COLUMN source_subscription_id UUID,
  ADD COLUMN external_revision TEXT,
  ADD COLUMN external_modified_at TIMESTAMPTZ;

UPDATE policy_imports
SET processing_object_key = raw_object_key,
    processing_content_sha256 = content_sha256,
    processing_mime_type = detected_mime_type;

ALTER TABLE policy_imports
  ALTER COLUMN processing_object_key SET NOT NULL,
  ALTER COLUMN processing_content_sha256 SET NOT NULL,
  ALTER COLUMN processing_mime_type SET NOT NULL;

CREATE TABLE policy_collections (
  id UUID PRIMARY KEY,
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  key VARCHAR(255) NOT NULL,
  version INTEGER NOT NULL CHECK (version > 0),
  title TEXT NOT NULL,
  status VARCHAR(32) NOT NULL CHECK (status IN ('draft', 'compiled')),
  compiled_policy_pack_id UUID REFERENCES policy_packs(id) ON DELETE SET NULL,
  created_by TEXT NOT NULL,
  idempotency_key VARCHAR(255),
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL,
  UNIQUE (organization_id, key, version)
);
CREATE UNIQUE INDEX uq_policy_collections_idempotency
  ON policy_collections (organization_id, idempotency_key)
  WHERE idempotency_key IS NOT NULL;
CREATE INDEX idx_policy_collections_tenant_updated
  ON policy_collections (organization_id, updated_at DESC);

CREATE TABLE source_connections (
  id UUID PRIMARY KEY,
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  provider VARCHAR(32) NOT NULL CHECK (provider IN ('google_drive', 'microsoft_graph', 'notion')),
  connected_by TEXT NOT NULL,
  provider_account_id TEXT NOT NULL,
  display_label TEXT NOT NULL,
  status VARCHAR(32) NOT NULL CHECK (status IN ('active', 'reauthorization_required', 'disconnected')),
  granted_scopes JSONB NOT NULL DEFAULT '[]'::jsonb,
  credential_ciphertext BYTEA,
  credential_nonce BYTEA,
  credential_key_version INTEGER,
  access_expires_at TIMESTAMPTZ,
  last_sync_at TIMESTAMPTZ,
  last_failure_code VARCHAR(255),
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL,
  UNIQUE (organization_id, provider, provider_account_id, connected_by),
  CHECK ((credential_ciphertext IS NULL AND credential_nonce IS NULL AND credential_key_version IS NULL)
      OR (credential_ciphertext IS NOT NULL AND octet_length(credential_nonce) = 12 AND credential_key_version > 0))
);
CREATE INDEX idx_source_connections_tenant_provider
  ON source_connections (organization_id, provider, status);

CREATE TABLE source_connection_oauth_states (
  state_hash VARCHAR(64) PRIMARY KEY,
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  provider VARCHAR(32) NOT NULL,
  actor_id TEXT NOT NULL,
  originating_collection_id UUID REFERENCES policy_collections(id) ON DELETE CASCADE,
  pkce_ciphertext BYTEA,
  pkce_nonce BYTEA,
  key_version INTEGER,
  redirect_uri TEXT NOT NULL,
  expires_at TIMESTAMPTZ NOT NULL,
  consumed_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_source_oauth_states_expiry
  ON source_connection_oauth_states (expires_at) WHERE consumed_at IS NULL;

CREATE TABLE source_subscriptions (
  id UUID PRIMARY KEY,
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  connection_id UUID REFERENCES source_connections(id) ON DELETE SET NULL,
  provider VARCHAR(32) NOT NULL,
  external_item_id TEXT NOT NULL,
  canonical_url TEXT,
  title TEXT NOT NULL,
  mime_type VARCHAR(255),
  policy_source_id UUID NOT NULL,
  last_external_revision TEXT,
  last_import_id UUID REFERENCES policy_imports(id) ON DELETE SET NULL,
  last_observed_modified_at TIMESTAMPTZ,
  status VARCHAR(40) NOT NULL,
  failure_code VARCHAR(255),
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL,
  UNIQUE (organization_id, connection_id, external_item_id)
);
CREATE INDEX idx_source_subscriptions_tenant_status
  ON source_subscriptions (organization_id, status, updated_at DESC);

CREATE TABLE source_ingestion_batches (
  id UUID PRIMARY KEY,
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  policy_collection_id UUID REFERENCES policy_collections(id) ON DELETE CASCADE,
  kind VARCHAR(32) NOT NULL,
  status VARCHAR(32) NOT NULL,
  requested_by TEXT NOT NULL,
  total_count INTEGER NOT NULL DEFAULT 0 CHECK (total_count >= 0),
  succeeded_count INTEGER NOT NULL DEFAULT 0 CHECK (succeeded_count >= 0),
  failed_count INTEGER NOT NULL DEFAULT 0 CHECK (failed_count >= 0),
  unchanged_count INTEGER NOT NULL DEFAULT 0 CHECK (unchanged_count >= 0),
  idempotency_key VARCHAR(255),
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL
);
CREATE UNIQUE INDEX uq_source_ingestion_batches_idempotency
  ON source_ingestion_batches (organization_id, idempotency_key)
  WHERE idempotency_key IS NOT NULL;
CREATE INDEX idx_source_ingestion_batches_collection
  ON source_ingestion_batches (organization_id, policy_collection_id, created_at DESC);

CREATE TABLE source_ingestion_items (
  id UUID PRIMARY KEY,
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  batch_id UUID NOT NULL REFERENCES source_ingestion_batches(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  client_item_key TEXT NOT NULL,
  connection_id UUID REFERENCES source_connections(id) ON DELETE SET NULL,
  subscription_id UUID REFERENCES source_subscriptions(id) ON DELETE SET NULL,
  external_item_id TEXT,
  status VARCHAR(32) NOT NULL,
  policy_import_id UUID REFERENCES policy_imports(id) ON DELETE SET NULL,
  failure_code VARCHAR(255),
  failure_detail TEXT,
  attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  created_at TIMESTAMPTZ NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL,
  UNIQUE (batch_id, client_item_key)
);
CREATE INDEX idx_source_ingestion_items_claim
  ON source_ingestion_items (organization_id, status, updated_at);

CREATE TABLE policy_import_transformations (
  id UUID PRIMARY KEY,
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  policy_import_id UUID NOT NULL REFERENCES policy_imports(id) ON DELETE CASCADE,
  kind VARCHAR(32) NOT NULL,
  input_object_key TEXT NOT NULL,
  input_sha256 VARCHAR(64) NOT NULL,
  output_object_key TEXT NOT NULL,
  output_sha256 VARCHAR(64) NOT NULL,
  output_mime_type VARCHAR(255) NOT NULL,
  processor TEXT NOT NULL,
  processor_version TEXT NOT NULL,
  created_by TEXT NOT NULL,
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL
);
CREATE UNIQUE INDEX uq_policy_import_active_manual_ocr
  ON policy_import_transformations (organization_id, policy_import_id, kind)
  WHERE kind = 'manual_ocr';

CREATE TABLE policy_collection_imports (
  organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  policy_collection_id UUID NOT NULL REFERENCES policy_collections(id) ON DELETE CASCADE,
  policy_import_id UUID NOT NULL REFERENCES policy_imports(id) ON DELETE RESTRICT,
  policy_source_id UUID NOT NULL,
  position INTEGER NOT NULL CHECK (position >= 0),
  added_at TIMESTAMPTZ NOT NULL,
  PRIMARY KEY (policy_collection_id, policy_import_id),
  UNIQUE (policy_collection_id, policy_source_id),
  UNIQUE (policy_collection_id, position)
);
CREATE INDEX idx_policy_collection_imports_tenant
  ON policy_collection_imports (organization_id, policy_collection_id, position);

ALTER TABLE policy_imports
  ADD CONSTRAINT fk_policy_imports_active_transformation
    FOREIGN KEY (active_transformation_id) REFERENCES policy_import_transformations(id) ON DELETE SET NULL,
  ADD CONSTRAINT fk_policy_imports_ingestion_item
    FOREIGN KEY (ingestion_item_id) REFERENCES source_ingestion_items(id) ON DELETE SET NULL,
  ADD CONSTRAINT fk_policy_imports_subscription
    FOREIGN KEY (source_subscription_id) REFERENCES source_subscriptions(id) ON DELETE SET NULL;

CREATE UNIQUE INDEX uq_policy_imports_ingestion_item
  ON policy_imports (organization_id, ingestion_item_id) WHERE ingestion_item_id IS NOT NULL;
CREATE INDEX idx_policy_imports_subscription_revision
  ON policy_imports (organization_id, source_subscription_id, external_revision);
",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r"
DROP INDEX IF EXISTS idx_policy_imports_subscription_revision;
DROP INDEX IF EXISTS uq_policy_imports_ingestion_item;
ALTER TABLE policy_imports
  DROP CONSTRAINT IF EXISTS fk_policy_imports_subscription,
  DROP CONSTRAINT IF EXISTS fk_policy_imports_ingestion_item,
  DROP CONSTRAINT IF EXISTS fk_policy_imports_active_transformation;
DROP TABLE IF EXISTS policy_collection_imports;
DROP TABLE IF EXISTS policy_import_transformations;
DROP TABLE IF EXISTS source_ingestion_items;
DROP TABLE IF EXISTS source_ingestion_batches;
DROP TABLE IF EXISTS source_subscriptions;
DROP TABLE IF EXISTS source_connection_oauth_states;
DROP TABLE IF EXISTS source_connections;
DROP TABLE IF EXISTS policy_collections;
ALTER TABLE policy_imports
  DROP COLUMN IF EXISTS external_modified_at,
  DROP COLUMN IF EXISTS external_revision,
  DROP COLUMN IF EXISTS source_subscription_id,
  DROP COLUMN IF EXISTS ingestion_item_id,
  DROP COLUMN IF EXISTS active_transformation_id,
  DROP COLUMN IF EXISTS processing_mime_type,
  DROP COLUMN IF EXISTS processing_content_sha256,
  DROP COLUMN IF EXISTS processing_object_key;
",
            )
            .await?;
        Ok(())
    }
}

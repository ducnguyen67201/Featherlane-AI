use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r"
ALTER TABLE policy_imports
    ADD COLUMN policy_source_id uuid,
    ADD COLUMN revision integer,
    ADD COLUMN supersedes_import_id uuid;

UPDATE policy_imports
SET policy_source_id = id,
    revision = 1;

ALTER TABLE policy_imports
    ALTER COLUMN policy_source_id SET NOT NULL,
    ALTER COLUMN revision SET NOT NULL,
    ADD CONSTRAINT fk_policy_imports_supersedes
        FOREIGN KEY (supersedes_import_id)
        REFERENCES policy_imports(id)
        ON DELETE RESTRICT;

CREATE UNIQUE INDEX uq_policy_imports_source_revision
    ON policy_imports (organization_id, policy_source_id, revision);
CREATE INDEX idx_policy_imports_source_history
    ON policy_imports (organization_id, policy_source_id, created_at DESC);
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
DROP INDEX IF EXISTS idx_policy_imports_source_history;
DROP INDEX IF EXISTS uq_policy_imports_source_revision;
ALTER TABLE policy_imports
    DROP CONSTRAINT IF EXISTS fk_policy_imports_supersedes,
    DROP COLUMN IF EXISTS supersedes_import_id,
    DROP COLUMN IF EXISTS revision,
    DROP COLUMN IF EXISTS policy_source_id;
",
            )
            .await?;
        Ok(())
    }
}

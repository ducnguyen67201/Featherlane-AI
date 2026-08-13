use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("uq_targets_org_key_version")
                    .table(Targets::Table)
                    .col(Targets::OrganizationId)
                    .col(Targets::Key)
                    .col(Targets::Version)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("uq_targets_org_key_version")
                    .table(Targets::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Targets {
    Table,
    OrganizationId,
    Key,
    Version,
}

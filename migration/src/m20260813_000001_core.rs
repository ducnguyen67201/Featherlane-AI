use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[allow(clippy::too_many_lines)] // The migration is intentionally reviewed as one atomic schema unit.
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Organizations::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Organizations::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Organizations::Name).string().not_null())
                    .col(
                        ColumnDef::new(Organizations::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        create_tenant_payload_table(
            manager,
            Sources::Table,
            Sources::Id,
            Sources::OrganizationId,
            Sources::Payload,
            Sources::CreatedAt,
        )
        .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Sources::Table)
                    .add_column(ColumnDef::new(Sources::SourceType).string().not_null())
                    .add_column(ColumnDef::new(Sources::Title).string().not_null())
                    .add_column(ColumnDef::new(Sources::Jurisdiction).string().not_null())
                    .add_column(ColumnDef::new(Sources::ContentSha256).string().not_null())
                    .add_column(ColumnDef::new(Sources::Confidence).string().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(PolicyPacks::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PolicyPacks::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(PolicyPacks::OrganizationId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PolicyPacks::Key).string().not_null())
                    .col(ColumnDef::new(PolicyPacks::Version).integer().not_null())
                    .col(ColumnDef::new(PolicyPacks::Title).string().not_null())
                    .col(ColumnDef::new(PolicyPacks::Status).string().not_null())
                    .col(
                        ColumnDef::new(PolicyPacks::ContentSha256)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PolicyPacks::PublishedAt).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(PolicyPacks::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(&mut organization_foreign_key(
                        "fk_policy_packs_organization",
                        PolicyPacks::OrganizationId,
                    ))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(PolicyRules::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PolicyRules::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(PolicyRules::OrganizationId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PolicyRules::PolicyPackId).uuid().not_null())
                    .col(ColumnDef::new(PolicyRules::RuleId).string().not_null())
                    .col(
                        ColumnDef::new(PolicyRules::RuleVersion)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PolicyRules::Position).integer().not_null())
                    .col(
                        ColumnDef::new(PolicyRules::ObligationKey)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PolicyRules::Severity).string().not_null())
                    .col(
                        ColumnDef::new(PolicyRules::RulePayload)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PolicyRules::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(&mut organization_foreign_key(
                        "fk_policy_rules_organization",
                        PolicyRules::OrganizationId,
                    ))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_policy_rules_pack")
                            .from(PolicyRules::Table, PolicyRules::PolicyPackId)
                            .to(PolicyPacks::Table, PolicyPacks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uq_policy_rule_version")
                    .table(PolicyRules::Table)
                    .col(PolicyRules::PolicyPackId)
                    .col(PolicyRules::RuleId)
                    .col(PolicyRules::RuleVersion)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Obligations::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Obligations::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Obligations::OrganizationId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Obligations::SourceId).uuid().not_null())
                    .col(ColumnDef::new(Obligations::Key).string().not_null())
                    .col(ColumnDef::new(Obligations::Statement).text().not_null())
                    .col(
                        ColumnDef::new(Obligations::ObligationPayload)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Obligations::ReviewStatus)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Obligations::ReviewerId).string())
                    .col(ColumnDef::new(Obligations::ReviewedAt).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(Obligations::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(&mut organization_foreign_key(
                        "fk_obligations_organization",
                        Obligations::OrganizationId,
                    ))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_obligations_source")
                            .from(Obligations::Table, Obligations::SourceId)
                            .to(Sources::Table, Sources::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uq_obligation_source_key")
                    .table(Obligations::Table)
                    .col(Obligations::SourceId)
                    .col(Obligations::Key)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(PolicyPackSources::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PolicyPackSources::PolicyPackId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PolicyPackSources::SourceId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PolicyPackSources::OrganizationId)
                            .uuid()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(PolicyPackSources::PolicyPackId)
                            .col(PolicyPackSources::SourceId),
                    )
                    .foreign_key(&mut organization_foreign_key(
                        "fk_policy_pack_sources_organization",
                        PolicyPackSources::OrganizationId,
                    ))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_policy_pack_sources_pack")
                            .from(PolicyPackSources::Table, PolicyPackSources::PolicyPackId)
                            .to(PolicyPacks::Table, PolicyPacks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_policy_pack_sources_source")
                            .from(PolicyPackSources::Table, PolicyPackSources::SourceId)
                            .to(Sources::Table, Sources::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(PolicyReviews::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PolicyReviews::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(PolicyReviews::OrganizationId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PolicyReviews::PolicyPackId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PolicyReviews::Status).string().not_null())
                    .col(
                        ColumnDef::new(PolicyReviews::ReviewerId)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PolicyReviews::Notes).text().not_null())
                    .col(
                        ColumnDef::new(PolicyReviews::ReviewedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(&mut organization_foreign_key(
                        "fk_policy_reviews_organization",
                        PolicyReviews::OrganizationId,
                    ))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_policy_reviews_pack")
                            .from(PolicyReviews::Table, PolicyReviews::PolicyPackId)
                            .to(PolicyPacks::Table, PolicyPacks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uq_policy_pack_version")
                    .table(PolicyPacks::Table)
                    .col(PolicyPacks::OrganizationId)
                    .col(PolicyPacks::Key)
                    .col(PolicyPacks::Version)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Targets::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Targets::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Targets::OrganizationId).uuid().not_null())
                    .col(ColumnDef::new(Targets::Key).string().not_null())
                    .col(ColumnDef::new(Targets::Version).string().not_null())
                    .col(ColumnDef::new(Targets::DriverType).string().not_null())
                    .col(ColumnDef::new(Targets::Endpoint).string().not_null())
                    .col(
                        ColumnDef::new(Targets::Capabilities)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Targets::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(&mut organization_foreign_key(
                        "fk_targets_organization",
                        Targets::OrganizationId,
                    ))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(EvalRuns::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(EvalRuns::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(EvalRuns::OrganizationId).uuid().not_null())
                    .col(ColumnDef::new(EvalRuns::TargetId).string().not_null())
                    .col(ColumnDef::new(EvalRuns::PolicyPackKey).string().not_null())
                    .col(ColumnDef::new(EvalRuns::Verdict).string().not_null())
                    .col(ColumnDef::new(EvalRuns::Summary).json_binary().not_null())
                    .col(
                        ColumnDef::new(EvalRuns::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(ColumnDef::new(EvalRuns::CompletedAt).timestamp_with_time_zone())
                    .foreign_key(&mut organization_foreign_key(
                        "fk_eval_runs_organization",
                        EvalRuns::OrganizationId,
                    ))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(NormalizedEvents::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NormalizedEvents::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NormalizedEvents::OrganizationId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NormalizedEvents::EvalRunId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NormalizedEvents::InvocationId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NormalizedEvents::TraceId)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NormalizedEvents::SpanId).string())
                    .col(
                        ColumnDef::new(NormalizedEvents::Sequence)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NormalizedEvents::EventType)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NormalizedEvents::Name).string().not_null())
                    .col(
                        ColumnDef::new(NormalizedEvents::Payload)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NormalizedEvents::StartedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_normalized_events_run")
                            .from(NormalizedEvents::Table, NormalizedEvents::EvalRunId)
                            .to(EvalRuns::Table, EvalRuns::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uq_normalized_event_span")
                    .table(NormalizedEvents::Table)
                    .col(NormalizedEvents::OrganizationId)
                    .col(NormalizedEvents::TraceId)
                    .col(NormalizedEvents::SpanId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RuleResults::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(RuleResults::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(RuleResults::OrganizationId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(RuleResults::EvalRunId).uuid().not_null())
                    .col(ColumnDef::new(RuleResults::RuleId).string().not_null())
                    .col(ColumnDef::new(RuleResults::Severity).string().not_null())
                    .col(ColumnDef::new(RuleResults::Status).string().not_null())
                    .col(
                        ColumnDef::new(RuleResults::Payload)
                            .json_binary()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(RuleResults::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_rule_results_run")
                            .from(RuleResults::Table, RuleResults::EvalRunId)
                            .to(EvalRuns::Table, EvalRuns::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Jobs::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Jobs::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Jobs::OrganizationId).uuid().not_null())
                    .col(ColumnDef::new(Jobs::Kind).string().not_null())
                    .col(ColumnDef::new(Jobs::Status).string().not_null())
                    .col(ColumnDef::new(Jobs::Payload).json_binary().not_null())
                    .col(
                        ColumnDef::new(Jobs::Attempts)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Jobs::AvailableAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Jobs::LeaseExpiresAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Jobs::LastError).text())
                    .col(
                        ColumnDef::new(Jobs::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(&mut organization_foreign_key(
                        "fk_jobs_organization",
                        Jobs::OrganizationId,
                    ))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_jobs_claim")
                    .table(Jobs::Table)
                    .col(Jobs::Status)
                    .col(Jobs::AvailableAt)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            Jobs::Table.into_iden(),
            RuleResults::Table.into_iden(),
            NormalizedEvents::Table.into_iden(),
            EvalRuns::Table.into_iden(),
            Targets::Table.into_iden(),
            PolicyReviews::Table.into_iden(),
            PolicyPackSources::Table.into_iden(),
            Obligations::Table.into_iden(),
            PolicyRules::Table.into_iden(),
            PolicyPacks::Table.into_iden(),
            Sources::Table.into_iden(),
            Organizations::Table.into_iden(),
        ] {
            manager
                .drop_table(Table::drop().table(table).if_exists().to_owned())
                .await?;
        }
        Ok(())
    }
}

async fn create_tenant_payload_table<T, I, O, P, C>(
    manager: &SchemaManager<'_>,
    table: T,
    id: I,
    organization_id: O,
    payload: P,
    created_at: C,
) -> Result<(), DbErr>
where
    T: IntoIden,
    I: IntoIden,
    O: IntoIden + Clone,
    P: IntoIden,
    C: IntoIden,
{
    manager
        .create_table(
            Table::create()
                .table(table)
                .if_not_exists()
                .col(ColumnDef::new(id).uuid().not_null().primary_key())
                .col(ColumnDef::new(organization_id.clone()).uuid().not_null())
                .col(ColumnDef::new(payload).json_binary().not_null())
                .col(
                    ColumnDef::new(created_at)
                        .timestamp_with_time_zone()
                        .not_null(),
                )
                .foreign_key(&mut organization_foreign_key(
                    "fk_sources_organization",
                    organization_id,
                ))
                .to_owned(),
        )
        .await
}

fn organization_foreign_key<N, C>(name: N, column: C) -> ForeignKeyCreateStatement
where
    N: Into<String>,
    C: IntoIden,
{
    ForeignKey::create()
        .name(name)
        .from_col(column)
        .to(Organizations::Table, Organizations::Id)
        .on_delete(ForeignKeyAction::Cascade)
        .on_update(ForeignKeyAction::Cascade)
        .to_owned()
}

#[derive(DeriveIden)]
enum Organizations {
    Table,
    Id,
    Name,
    CreatedAt,
}

#[derive(Clone, DeriveIden)]
enum Sources {
    Table,
    Id,
    OrganizationId,
    SourceType,
    Title,
    Jurisdiction,
    ContentSha256,
    Confidence,
    Payload,
    CreatedAt,
}

#[derive(DeriveIden)]
enum PolicyPacks {
    Table,
    Id,
    OrganizationId,
    Key,
    Version,
    Title,
    Status,
    ContentSha256,
    PublishedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum PolicyRules {
    Table,
    Id,
    OrganizationId,
    PolicyPackId,
    RuleId,
    RuleVersion,
    Position,
    ObligationKey,
    Severity,
    RulePayload,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Obligations {
    Table,
    Id,
    OrganizationId,
    SourceId,
    Key,
    Statement,
    ObligationPayload,
    ReviewStatus,
    ReviewerId,
    ReviewedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum PolicyPackSources {
    Table,
    PolicyPackId,
    SourceId,
    OrganizationId,
}

#[derive(DeriveIden)]
enum PolicyReviews {
    Table,
    Id,
    OrganizationId,
    PolicyPackId,
    Status,
    ReviewerId,
    Notes,
    ReviewedAt,
}

#[derive(DeriveIden)]
enum Targets {
    Table,
    Id,
    OrganizationId,
    Key,
    Version,
    DriverType,
    Endpoint,
    Capabilities,
    CreatedAt,
}

#[derive(DeriveIden)]
enum EvalRuns {
    Table,
    Id,
    OrganizationId,
    TargetId,
    PolicyPackKey,
    Verdict,
    Summary,
    CreatedAt,
    CompletedAt,
}

#[derive(DeriveIden)]
enum NormalizedEvents {
    Table,
    Id,
    OrganizationId,
    EvalRunId,
    InvocationId,
    TraceId,
    SpanId,
    Sequence,
    EventType,
    Name,
    Payload,
    StartedAt,
}

#[derive(DeriveIden)]
enum RuleResults {
    Table,
    Id,
    OrganizationId,
    EvalRunId,
    RuleId,
    Severity,
    Status,
    Payload,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Jobs {
    Table,
    Id,
    OrganizationId,
    Kind,
    Status,
    Payload,
    Attempts,
    AvailableAt,
    LeaseExpiresAt,
    LastError,
    CreatedAt,
}

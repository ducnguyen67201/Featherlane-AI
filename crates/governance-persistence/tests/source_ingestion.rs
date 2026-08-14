use governance_application::SourceIngestionRepository;
use governance_domain::{
    OrganizationId, SourceIngestionBatch, SourceIngestionBatchId, SourceIngestionBatchKind,
    SourceIngestionBatchStatus, SourceIngestionItem, SourceIngestionItemId,
    SourceIngestionItemStatus,
};
use governance_migration::Migrator;
use governance_persistence::SeaOrmSourceIngestionRepository;
use sea_orm::{ConnectionTrait, Database, Statement};
use sea_orm_migration::MigratorTrait;
use time::OffsetDateTime;

#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::too_many_lines)]
async fn batch_claim_is_single_owner_and_aggregation_preserves_partial_success() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let database = Database::connect(database_url)
        .await
        .expect("test database should connect");
    Migrator::up(&database, None)
        .await
        .expect("migrations should apply");
    let repository = SeaOrmSourceIngestionRepository::new(database.clone());
    let organization_id = OrganizationId::new();
    let batch_id = SourceIngestionBatchId::new();
    let now = OffsetDateTime::now_utc();
    let items: Vec<_> = (0..3)
        .map(|ordinal| SourceIngestionItem {
            id: SourceIngestionItemId::new(),
            organization_id,
            batch_id,
            ordinal,
            client_item_key: format!("item-{ordinal}"),
            connection_id: None,
            subscription_id: None,
            external_item_id: Some(format!("https://example.test/{ordinal}")),
            status: SourceIngestionItemStatus::Pending,
            policy_import_id: None,
            failure_code: None,
            failure_detail: None,
            attempt_count: 0,
            created_at: now,
            updated_at: now,
        })
        .collect();
    let batch = SourceIngestionBatch {
        id: batch_id,
        organization_id,
        policy_collection_id: None,
        kind: SourceIngestionBatchKind::Url,
        status: SourceIngestionBatchStatus::Pending,
        requested_by: "repository-test".to_owned(),
        total_count: 3,
        succeeded_count: 0,
        failed_count: 0,
        unchanged_count: 0,
        created_at: now,
        updated_at: now,
    };
    repository
        .create_batch(&batch, &items)
        .await
        .expect("batch should persist");
    repository
        .claim_item(organization_id, items[0].id)
        .await
        .expect("first claim should succeed");
    assert!(
        repository
            .claim_item(organization_id, items[0].id)
            .await
            .is_err()
    );
    repository
        .update_item(
            organization_id,
            items[0].id,
            SourceIngestionItemStatus::Queued,
            None,
            None,
        )
        .await
        .expect("queued transition");
    repository
        .update_item(
            organization_id,
            items[0].id,
            SourceIngestionItemStatus::Processing,
            None,
            None,
        )
        .await
        .expect("processing transition");
    repository
        .update_item(
            organization_id,
            items[0].id,
            SourceIngestionItemStatus::ReviewRequired,
            None,
            None,
        )
        .await
        .expect("review transition");
    repository
        .claim_item(organization_id, items[1].id)
        .await
        .expect("claim unchanged item");
    repository
        .update_item(
            organization_id,
            items[1].id,
            SourceIngestionItemStatus::Unchanged,
            None,
            None,
        )
        .await
        .expect("unchanged transition");
    repository
        .claim_item(organization_id, items[2].id)
        .await
        .expect("claim failed item");
    repository
        .update_item(
            organization_id,
            items[2].id,
            SourceIngestionItemStatus::Failed,
            None,
            Some(("download_failed", "provider unavailable")),
        )
        .await
        .expect("failed transition");
    let aggregate = repository
        .recompute_batch(organization_id, batch_id)
        .await
        .expect("batch should aggregate");
    assert_eq!(aggregate.status, SourceIngestionBatchStatus::Partial);
    assert_eq!(
        (
            aggregate.succeeded_count,
            aggregate.unchanged_count,
            aggregate.failed_count
        ),
        (1, 1, 1)
    );

    for statement in [
        format!("DELETE FROM source_ingestion_items WHERE organization_id='{organization_id}'"),
        format!("DELETE FROM source_ingestion_batches WHERE organization_id='{organization_id}'"),
        format!("DELETE FROM organizations WHERE id='{organization_id}'"),
    ] {
        database
            .execute_raw(Statement::from_string(
                database.get_database_backend(),
                statement,
            ))
            .await
            .expect("test cleanup should succeed");
    }
}

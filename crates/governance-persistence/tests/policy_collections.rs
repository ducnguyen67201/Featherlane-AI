use governance_application::{PolicyCollectionRepository, PolicyImportRepository};
use governance_domain::{
    OrganizationId, PolicyCollection, PolicyCollectionId, PolicyCollectionStatus, PolicyImport,
    PolicyImportCoverage, PolicyImportId, PolicyImportStatus, PolicyInputKind, PolicySourceId,
    SourceType, SourceVerificationStatus,
};
use governance_migration::Migrator;
use governance_persistence::{SeaOrmPolicyCollectionRepository, SeaOrmPolicyImportRepository};
use sea_orm::{ConnectionTrait, Database, Statement};
use sea_orm_migration::MigratorTrait;
use time::OffsetDateTime;

#[tokio::test(flavor = "multi_thread")]
async fn a_collection_accepts_only_one_revision_of_a_policy_source() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let database = Database::connect(database_url)
        .await
        .expect("test database should connect");
    Migrator::up(&database, None)
        .await
        .expect("migrations should apply");
    let organization_id = OrganizationId::new();
    let imports = SeaOrmPolicyImportRepository::new(database.clone());
    let collections = SeaOrmPolicyCollectionRepository::new(database.clone());
    let first = policy_import(organization_id, PolicySourceId::new(), 1, None);
    let second = policy_import(organization_id, first.policy_source_id, 2, Some(first.id));
    imports
        .create(&first)
        .await
        .expect("first import should persist");
    imports
        .create(&second)
        .await
        .expect("second import should persist");
    let now = OffsetDateTime::now_utc();
    let collection = PolicyCollection {
        id: PolicyCollectionId::new(),
        organization_id,
        key: format!("collection-{}", PolicyCollectionId::new()),
        version: 1,
        title: "Collection membership invariant".to_owned(),
        status: PolicyCollectionStatus::Draft,
        compiled_policy_pack_id: None,
        created_by: "repository-test".to_owned(),
        idempotency_key: None,
        created_at: now,
        updated_at: now,
    };
    collections
        .create(&collection)
        .await
        .expect("collection should persist");
    collections
        .add_import(organization_id, collection.id, &first)
        .await
        .expect("first revision should join");
    assert!(
        collections
            .add_import(organization_id, collection.id, &second)
            .await
            .is_err()
    );

    cleanup(&database, organization_id).await;
}

async fn cleanup(database: &sea_orm::DatabaseConnection, organization_id: OrganizationId) {
    for statement in [
        format!("DELETE FROM policy_collection_imports WHERE organization_id='{organization_id}'"),
        format!("DELETE FROM policy_collections WHERE organization_id='{organization_id}'"),
        format!("DELETE FROM policy_imports WHERE organization_id='{organization_id}'"),
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

fn policy_import(
    organization_id: OrganizationId,
    policy_source_id: PolicySourceId,
    revision: u32,
    supersedes_import_id: Option<PolicyImportId>,
) -> PolicyImport {
    let id = PolicyImportId::new();
    let now = OffsetDateTime::now_utc();
    PolicyImport {
        id,
        organization_id,
        policy_source_id,
        revision,
        supersedes_import_id,
        status: PolicyImportStatus::Uploading,
        input_kind: PolicyInputKind::PastedText,
        source_type: SourceType::CompanyPolicy,
        title: format!("Source revision {revision}"),
        jurisdiction: "internal".to_owned(),
        effective_from: None,
        source_url: None,
        original_filename: None,
        declared_mime_type: Some("text/plain".to_owned()),
        detected_mime_type: "text/plain".to_owned(),
        byte_length: 6,
        content_sha256: format!("sha-{id}"),
        raw_object_key: format!("test/{id}/raw"),
        processing_object_key: format!("test/{id}/raw"),
        processing_content_sha256: format!("sha-{id}"),
        processing_mime_type: "text/plain".to_owned(),
        active_transformation_id: None,
        ingestion_item_id: None,
        source_subscription_id: None,
        external_revision: None,
        external_modified_at: None,
        normalized_object_key: None,
        parser_kind: None,
        parser_version: None,
        model_provider: None,
        model_name: None,
        prompt_version: None,
        page_count: None,
        coverage: PolicyImportCoverage::default(),
        candidate_count: 0,
        verification_status: SourceVerificationStatus::Pending,
        verified_by: None,
        verified_at: None,
        verification_notes: None,
        failure_code: None,
        failure_detail: None,
        idempotency_key: Some(format!("collection-revision-{id}")),
        compiled_source_id: None,
        compiled_policy_pack_id: None,
        created_at: now,
        updated_at: now,
        completed_at: None,
    }
}

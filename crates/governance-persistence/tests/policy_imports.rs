use governance_application::PolicyImportRepository;
use governance_domain::{
    OrganizationId, PolicyImport, PolicyImportCoverage, PolicyImportId, PolicyImportStatus,
    PolicyInputKind, PolicySourceId, SourceType, SourceVerificationStatus,
};
use governance_migration::Migrator;
use governance_persistence::SeaOrmPolicyImportRepository;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;
use time::{Duration, OffsetDateTime};

struct PolicyImportCleanup {
    database: DatabaseConnection,
    organization_id: OrganizationId,
    active: bool,
}

impl Drop for PolicyImportCleanup {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let database = self.database.clone();
        let organization_id = self.organization_id;
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                if let Err(error) = cleanup_policy_imports(&database, organization_id).await {
                    eprintln!("policy import test cleanup failed: {error}");
                }
            });
        });
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn creating_multiple_imports_reuses_the_existing_organization() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let database = Database::connect(database_url)
        .await
        .expect("test database should connect");
    Migrator::up(&database, None)
        .await
        .expect("migrations should apply");

    let repository = SeaOrmPolicyImportRepository::new(database.clone());
    let organization_id = OrganizationId::new();
    let mut cleanup = PolicyImportCleanup {
        database: database.clone(),
        organization_id,
        active: true,
    };
    let mut first = policy_import(organization_id, "first");
    first.status = PolicyImportStatus::FailedTerminal;
    first.created_at -= Duration::days(1);
    first.updated_at = first.created_at;
    let mut second = policy_import(organization_id, "second");
    second.policy_source_id = first.policy_source_id;
    second.revision = 2;
    second.supersedes_import_id = Some(first.id);

    repository
        .create(&first)
        .await
        .expect("first import should create the organization");
    repository
        .create(&second)
        .await
        .expect("second import should reuse the organization");
    for index in 0..103 {
        repository
            .create(&policy_import(organization_id, &format!("import-{index}")))
            .await
            .expect("additional import should create");
    }

    let first_exists = repository
        .get(organization_id, first.id)
        .await
        .expect("first import lookup should succeed")
        .is_some();
    let loaded_second = repository
        .get(organization_id, second.id)
        .await
        .expect("second import lookup should succeed")
        .expect("second import should exist");
    let second_lineage = (
        loaded_second.policy_source_id,
        loaded_second.revision,
        loaded_second.supersedes_import_id,
    );
    let first_page = repository
        .list(organization_id, 100, None, None)
        .await
        .expect("first import page should load");
    let first_page_len = first_page.len();
    let second_page = repository
        .list(
            organization_id,
            100,
            None,
            first_page.last().map(|import| import.id),
        )
        .await
        .expect("second import page should load");
    let second_page_len = second_page.len();
    let pages_do_not_overlap = first_page
        .iter()
        .all(|first| second_page.iter().all(|second| first.id != second.id));
    let terminal_imports = repository
        .list(
            organization_id,
            25,
            Some(PolicyImportStatus::FailedTerminal),
            None,
        )
        .await
        .expect("status-filtered import page should load");
    let terminal_ids = terminal_imports
        .iter()
        .map(|import| import.id)
        .collect::<Vec<_>>();
    let second_exists = repository
        .get(organization_id, second.id)
        .await
        .expect("second import lookup should succeed")
        .is_some();

    cleanup_policy_imports(&database, organization_id)
        .await
        .expect("test records should clean up");
    cleanup.active = false;

    assert!(first_exists);
    assert_eq!(second_lineage, (first.policy_source_id, 2, Some(first.id)));
    assert_eq!(first_page_len, 100);
    assert_eq!(second_page_len, 5);
    assert!(pages_do_not_overlap);
    assert_eq!(terminal_ids, vec![first.id]);
    assert!(second_exists);
}

async fn cleanup_policy_imports(
    database: &DatabaseConnection,
    organization_id: OrganizationId,
) -> Result<(), sea_orm::DbErr> {
    for statement in [
        format!("DELETE FROM policy_imports WHERE organization_id='{organization_id}'"),
        format!("DELETE FROM organizations WHERE id='{organization_id}'"),
    ] {
        database
            .execute_raw(Statement::from_string(
                database.get_database_backend(),
                statement,
            ))
            .await?;
    }
    Ok(())
}

fn policy_import(organization_id: OrganizationId, title: &str) -> PolicyImport {
    let id = PolicyImportId::new();
    let now = OffsetDateTime::now_utc();
    PolicyImport {
        id,
        organization_id,
        policy_source_id: PolicySourceId::new(),
        revision: 1,
        supersedes_import_id: None,
        status: PolicyImportStatus::Uploading,
        input_kind: PolicyInputKind::PastedText,
        source_type: SourceType::CompanyPolicy,
        title: title.to_owned(),
        jurisdiction: "internal".to_owned(),
        effective_from: None,
        source_url: None,
        original_filename: None,
        declared_mime_type: Some("text/plain".to_owned()),
        detected_mime_type: "text/plain".to_owned(),
        byte_length: 6,
        content_sha256: format!("sha-{title}"),
        raw_object_key: format!("test/{organization_id}/{id}/raw"),
        processing_object_key: format!("test/{organization_id}/{id}/raw"),
        processing_content_sha256: format!("sha-{title}"),
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
        idempotency_key: Some(format!("policy-import-test-{id}")),
        compiled_source_id: None,
        compiled_policy_pack_id: None,
        created_at: now,
        updated_at: now,
        completed_at: None,
    }
}

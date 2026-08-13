use governance_application::PolicyImportRepository;
use governance_domain::{
    OrganizationId, PolicyImport, PolicyImportCoverage, PolicyImportId, PolicyImportStatus,
    PolicyInputKind, SourceType, SourceVerificationStatus,
};
use governance_migration::Migrator;
use governance_persistence::SeaOrmPolicyImportRepository;
use sea_orm::{ConnectionTrait, Database, Statement};
use sea_orm_migration::MigratorTrait;
use time::{Duration, OffsetDateTime};

#[tokio::test]
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
    let mut first = policy_import(organization_id, "first");
    first.status = PolicyImportStatus::FailedTerminal;
    first.created_at -= Duration::days(1);
    first.updated_at = first.created_at;
    let second = policy_import(organization_id, "second");

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

    assert!(
        repository
            .get(organization_id, first.id)
            .await
            .expect("first import lookup should succeed")
            .is_some()
    );
    let first_page = repository
        .list(organization_id, 100, None, None)
        .await
        .expect("first import page should load");
    assert_eq!(first_page.len(), 100);
    let second_page = repository
        .list(
            organization_id,
            100,
            None,
            first_page.last().map(|import| import.id),
        )
        .await
        .expect("second import page should load");
    assert_eq!(second_page.len(), 5);
    assert!(
        first_page
            .iter()
            .all(|first| second_page.iter().all(|second| first.id != second.id))
    );
    let terminal_imports = repository
        .list(
            organization_id,
            25,
            Some(PolicyImportStatus::FailedTerminal),
            None,
        )
        .await
        .expect("status-filtered import page should load");
    assert_eq!(terminal_imports.len(), 1);
    assert_eq!(terminal_imports[0].id, first.id);
    assert!(
        repository
            .get(organization_id, second.id)
            .await
            .expect("second import lookup should succeed")
            .is_some()
    );

    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!("DELETE FROM policy_imports WHERE organization_id='{organization_id}'"),
        ))
        .await
        .expect("test imports should clean up");
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!("DELETE FROM organizations WHERE id='{organization_id}'"),
        ))
        .await
        .expect("test organization should clean up");
}

fn policy_import(organization_id: OrganizationId, title: &str) -> PolicyImport {
    let id = PolicyImportId::new();
    let now = OffsetDateTime::now_utc();
    PolicyImport {
        id,
        organization_id,
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

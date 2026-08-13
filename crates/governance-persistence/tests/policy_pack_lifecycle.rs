use governance_application::PolicyPackRepository;
use governance_domain::{OrganizationId, PolicyPackId, PolicyPackStatusChange, ReviewStatus};
use governance_migration::Migrator;
use governance_persistence::SeaOrmPolicyPackRepository;
use sea_orm::{ConnectionTrait, Database, Statement};
use sea_orm_migration::MigratorTrait;
use time::OffsetDateTime;

#[tokio::test]
async fn approved_pack_can_be_disabled_and_enabled_without_losing_history() {
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
    let pack_id = PolicyPackId::new();
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!("INSERT INTO organizations(id,name,created_at) VALUES ('{organization_id}','pack lifecycle',now())"),
        ))
        .await
        .expect("organization fixture should insert");
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!(
                "INSERT INTO policy_packs(id,organization_id,key,version,title,status,content_sha256,published_at,created_at) \
                 VALUES ('{pack_id}','{organization_id}','lifecycle-pack',1,'Lifecycle','approved','sha',now(),now())"
            ),
        ))
        .await
        .expect("policy pack fixture should insert");

    let repository = SeaOrmPolicyPackRepository::new(database.clone());
    let disabled = repository
        .disable(
            organization_id,
            pack_id,
            &PolicyPackStatusChange {
                actor_id: "owner@example.com".to_owned(),
                notes: "Temporarily removed from evaluations".to_owned(),
                changed_at: OffsetDateTime::now_utc(),
            },
        )
        .await
        .expect("approved policy pack should disable");
    assert_eq!(disabled.status, ReviewStatus::Disabled);
    assert!(disabled.published_at.is_none());
    assert!(disabled.ensure_publishable().is_err());

    let enabled = repository
        .enable(
            organization_id,
            pack_id,
            &PolicyPackStatusChange {
                actor_id: "owner@example.com".to_owned(),
                notes: "Restored for evaluations".to_owned(),
                changed_at: OffsetDateTime::now_utc(),
            },
        )
        .await
        .expect("disabled policy pack should enable");
    assert_eq!(enabled.status, ReviewStatus::Approved);
    assert!(enabled.published_at.is_some());

    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!("DELETE FROM policy_reviews WHERE organization_id='{organization_id}'"),
        ))
        .await
        .expect("policy reviews should clean up");
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!("DELETE FROM policy_packs WHERE organization_id='{organization_id}'"),
        ))
        .await
        .expect("policy packs should clean up");
    database
        .execute_raw(Statement::from_string(
            database.get_database_backend(),
            format!("DELETE FROM organizations WHERE id='{organization_id}'"),
        ))
        .await
        .expect("organization should clean up");
}

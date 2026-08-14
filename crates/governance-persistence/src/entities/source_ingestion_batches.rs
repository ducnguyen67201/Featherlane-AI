use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "source_ingestion_batches")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub organization_id: Uuid,
    pub policy_collection_id: Option<Uuid>,
    pub kind: String,
    pub status: String,
    pub requested_by: String,
    pub total_count: i32,
    pub succeeded_count: i32,
    pub failed_count: i32,
    pub unchanged_count: i32,
    pub idempotency_key: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

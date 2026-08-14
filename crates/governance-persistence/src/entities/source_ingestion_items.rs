use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "source_ingestion_items")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub organization_id: Uuid,
    pub batch_id: Uuid,
    pub ordinal: i32,
    pub client_item_key: String,
    pub connection_id: Option<Uuid>,
    pub subscription_id: Option<Uuid>,
    pub external_item_id: Option<String>,
    pub status: String,
    pub policy_import_id: Option<Uuid>,
    pub failure_code: Option<String>,
    pub failure_detail: Option<String>,
    pub attempt_count: i32,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

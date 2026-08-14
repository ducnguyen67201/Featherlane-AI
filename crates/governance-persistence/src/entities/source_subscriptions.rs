use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "source_subscriptions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub organization_id: Uuid,
    pub connection_id: Option<Uuid>,
    pub provider: String,
    pub external_item_id: String,
    pub canonical_url: Option<String>,
    pub title: String,
    pub mime_type: Option<String>,
    pub policy_source_id: Uuid,
    pub last_external_revision: Option<String>,
    pub last_import_id: Option<Uuid>,
    pub last_observed_modified_at: Option<OffsetDateTime>,
    pub status: String,
    pub failure_code: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

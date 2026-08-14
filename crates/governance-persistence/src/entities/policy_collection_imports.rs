use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "policy_collection_imports")]
pub struct Model {
    pub organization_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub policy_collection_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub policy_import_id: Uuid,
    pub policy_source_id: Uuid,
    pub position: i32,
    pub added_at: OffsetDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

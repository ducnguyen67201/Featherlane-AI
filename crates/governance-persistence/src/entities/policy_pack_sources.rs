use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "policy_pack_sources")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub policy_pack_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub source_id: Uuid,
    pub organization_id: Uuid,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

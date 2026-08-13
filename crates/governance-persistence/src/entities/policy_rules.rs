use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "policy_rules")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub organization_id: Uuid,
    pub policy_pack_id: Uuid,
    pub rule_id: String,
    pub rule_version: i32,
    pub position: i32,
    pub obligation_key: String,
    pub severity: String,
    pub rule_payload: Json,
    pub created_at: OffsetDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::policy_packs::Entity",
        from = "Column::PolicyPackId",
        to = "super::policy_packs::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    PolicyPack,
}

impl Related<super::policy_packs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PolicyPack.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

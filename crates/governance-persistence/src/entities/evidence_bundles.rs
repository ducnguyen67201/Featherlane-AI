use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "evidence_bundles")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub organization_id: Uuid,
    pub eval_run_id: Uuid,
    pub schema_version: String,
    pub evidence_sha256: String,
    pub payload: Json,
    pub finalized_at: OffsetDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::eval_runs::Entity",
        from = "Column::EvalRunId",
        to = "super::eval_runs::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    EvalRun,
}

impl Related<super::eval_runs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EvalRun.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

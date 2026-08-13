use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "normalized_events")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub organization_id: Uuid,
    pub eval_run_id: Uuid,
    pub invocation_id: Uuid,
    pub trace_id: String,
    pub span_id: Option<String>,
    pub sequence: i64,
    pub event_type: String,
    pub name: String,
    pub payload: Json,
    pub started_at: OffsetDateTime,
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

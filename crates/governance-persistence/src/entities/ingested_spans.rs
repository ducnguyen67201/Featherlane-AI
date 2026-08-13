use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "ingested_spans")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub organization_id: Uuid,
    pub eval_run_id: Option<Uuid>,
    pub target_id: String,
    pub external_run_id: Option<String>,
    pub invocation_id: Option<Uuid>,
    pub scenario_id: Option<Uuid>,
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub links: Json,
    pub resource: Json,
    pub scope_name: Option<String>,
    pub scope_version: Option<String>,
    pub name: String,
    pub status: Option<String>,
    pub started_at: OffsetDateTime,
    pub ended_at: Option<OffsetDateTime>,
    pub attributes: Json,
    pub sanitized_payload_sha256: String,
    pub correlation_status: String,
    pub late_after_finalize: bool,
    pub received_at: OffsetDateTime,
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

use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "eval_runs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub organization_id: Uuid,
    pub target_id: String,
    pub target_version: Option<String>,
    pub policy_pack_key: String,
    pub policy_pack_id: Option<Uuid>,
    pub policy_pack_version: Option<i32>,
    pub policy_content_sha256: Option<String>,
    pub scenario_id: Option<Uuid>,
    pub rule_ids: Json,
    pub boundary_kind: String,
    pub external_run_id: Option<String>,
    pub primary_invocation_id: Option<Uuid>,
    pub state: String,
    pub completion_reason: Option<String>,
    pub terminal_state: Option<String>,
    pub verdict: Option<String>,
    pub summary: Option<Json>,
    pub settle_until: Option<OffsetDateTime>,
    pub hard_deadline_at: Option<OffsetDateTime>,
    pub last_seen_at: Option<OffsetDateTime>,
    pub finalized_at: Option<OffsetDateTime>,
    pub updated_at: OffsetDateTime,
    pub span_count: i64,
    pub trace_count: i64,
    pub event_count: i64,
    pub trace_quality: Option<String>,
    pub evidence_sha256: Option<String>,
    pub created_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::organizations::Entity",
        from = "Column::OrganizationId",
        to = "super::organizations::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Organization,
}

impl Related<super::organizations::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Organization.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

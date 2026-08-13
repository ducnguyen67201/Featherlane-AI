use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "policy_imports")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub organization_id: Uuid,
    pub policy_source_id: Uuid,
    pub revision: i32,
    pub supersedes_import_id: Option<Uuid>,
    pub status: String,
    pub input_kind: String,
    pub source_type: String,
    pub title: String,
    pub jurisdiction: String,
    pub effective_from: Option<OffsetDateTime>,
    pub source_url: Option<String>,
    pub original_filename: Option<String>,
    pub declared_mime_type: Option<String>,
    pub detected_mime_type: String,
    pub byte_length: i64,
    pub content_sha256: String,
    pub raw_object_key: String,
    pub normalized_object_key: Option<String>,
    pub parser_kind: Option<String>,
    pub parser_version: Option<String>,
    pub model_provider: Option<String>,
    pub model_name: Option<String>,
    pub prompt_version: Option<String>,
    pub page_count: Option<i32>,
    pub coverage_payload: Json,
    pub candidate_count: i32,
    pub verification_status: String,
    pub verified_by: Option<String>,
    pub verified_at: Option<OffsetDateTime>,
    pub verification_notes: Option<String>,
    pub failure_code: Option<String>,
    pub failure_detail: Option<String>,
    pub idempotency_key: Option<String>,
    pub compiled_source_id: Option<Uuid>,
    pub compiled_policy_pack_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::policy_candidates::Entity")]
    Candidates,
    #[sea_orm(
        belongs_to = "super::organizations::Entity",
        from = "Column::OrganizationId",
        to = "super::organizations::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Organization,
}

impl Related<super::policy_candidates::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Candidates.def()
    }
}

impl Related<super::organizations::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Organization.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

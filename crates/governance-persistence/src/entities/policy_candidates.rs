use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "policy_candidates")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub organization_id: Uuid,
    pub policy_import_id: Uuid,
    pub position: i32,
    pub origin: String,
    pub fingerprint: String,
    pub key: String,
    pub statement: String,
    pub locator_payload: Json,
    pub source_excerpt: String,
    pub applicability: Json,
    pub exceptions: Json,
    pub required_evidence: Json,
    pub suggested_severity: String,
    pub suggested_rule: Option<Json>,
    pub mapping_status: String,
    pub model_confidence: Option<f64>,
    pub model_payload_sha256: Option<String>,
    pub status: String,
    pub review_payload: Option<Json>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::policy_imports::Entity",
        from = "Column::PolicyImportId",
        to = "super::policy_imports::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    PolicyImport,
    #[sea_orm(has_many = "super::policy_candidate_reviews::Entity")]
    Reviews,
}

impl Related<super::policy_imports::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PolicyImport.def()
    }
}

impl Related<super::policy_candidate_reviews::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Reviews.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

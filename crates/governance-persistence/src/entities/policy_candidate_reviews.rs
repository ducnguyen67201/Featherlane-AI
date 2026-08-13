use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "policy_candidate_reviews")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub organization_id: Uuid,
    pub candidate_id: Uuid,
    pub decision: String,
    pub reviewer_id: String,
    pub notes: String,
    pub before_payload: Json,
    pub after_payload: Json,
    pub reviewed_at: OffsetDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::policy_candidates::Entity",
        from = "Column::CandidateId",
        to = "super::policy_candidates::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Candidate,
}

impl Related<super::policy_candidates::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Candidate.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "policy_import_transformations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub organization_id: Uuid,
    pub policy_import_id: Uuid,
    pub kind: String,
    pub input_object_key: String,
    pub input_sha256: String,
    pub output_object_key: String,
    pub output_sha256: String,
    pub output_mime_type: String,
    pub processor: String,
    pub processor_version: String,
    pub created_by: String,
    pub metadata: Json,
    pub created_at: OffsetDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

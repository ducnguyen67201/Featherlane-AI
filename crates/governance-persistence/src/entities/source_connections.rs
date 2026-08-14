use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "source_connections")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub organization_id: Uuid,
    pub provider: String,
    pub connected_by: String,
    pub provider_account_id: String,
    pub display_label: String,
    pub status: String,
    pub granted_scopes: Json,
    pub credential_ciphertext: Option<Vec<u8>>,
    pub credential_nonce: Option<Vec<u8>>,
    pub credential_key_version: Option<i32>,
    pub access_expires_at: Option<OffsetDateTime>,
    pub last_sync_at: Option<OffsetDateTime>,
    pub last_failure_code: Option<String>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

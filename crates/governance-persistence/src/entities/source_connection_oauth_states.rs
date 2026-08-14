use sea_orm::entity::prelude::*;
use time::OffsetDateTime;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "source_connection_oauth_states")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub state_hash: String,
    pub organization_id: Uuid,
    pub provider: String,
    pub actor_id: String,
    pub originating_collection_id: Option<Uuid>,
    pub pkce_ciphertext: Option<Vec<u8>>,
    pub pkce_nonce: Option<Vec<u8>>,
    pub key_version: Option<i32>,
    pub redirect_uri: String,
    pub expires_at: OffsetDateTime,
    pub consumed_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

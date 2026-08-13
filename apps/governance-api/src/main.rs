use governance_api::loco_app::App;
use governance_migration::Migrator;

#[tokio::main]
async fn main() -> loco_rs::Result<()> {
    loco_rs::cli::main::<App, Migrator>().await
}

use clap::{Parser, Subcommand};
use governance_migration::Migrator;
use sea_orm_migration::{MigratorTrait, sea_orm::Database};

#[derive(Debug, Parser)]
struct Arguments {
    #[arg(
        long,
        env = "DATABASE_URL",
        default_value = "postgres://featherlane:featherlane@localhost:5432/featherlane"
    )]
    database_url: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Up,
    Down {
        #[arg(long, default_value_t = 1)]
        steps: u32,
    },
    Status,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = Arguments::parse();
    let database = Database::connect(&arguments.database_url).await?;
    match arguments.command {
        Command::Up => Migrator::up(&database, None).await?,
        Command::Down { steps } => Migrator::down(&database, Some(steps)).await?,
        Command::Status => Migrator::status(&database).await?,
    }
    Ok(())
}

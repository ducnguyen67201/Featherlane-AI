use sea_orm_migration::prelude::*;

mod m20260813_000001_core;
mod m20260813_000002_target_versions;

#[derive(Debug)]
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260813_000001_core::Migration),
            Box::new(m20260813_000002_target_versions::Migration),
        ]
    }
}

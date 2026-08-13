use sea_orm_migration::prelude::*;

mod m20260813_000001_core;
mod m20260813_000002_policy_import_workflow;
mod m20260813_000003_correlated_evaluation_runs;
mod m20260813_000004_policy_source_lineage;

#[derive(Debug)]
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260813_000001_core::Migration),
            Box::new(m20260813_000002_policy_import_workflow::Migration),
            Box::new(m20260813_000003_correlated_evaluation_runs::Migration),
            Box::new(m20260813_000004_policy_source_lineage::Migration),
        ]
    }
}

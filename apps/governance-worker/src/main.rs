use governance_application::{
    ApplicationError, CompleteEvaluationRun, CompletionRequest, DurableJobRepository,
    EvaluateFinalizedRun, EvaluationRunRepository, FinalizeEvaluationRun,
    TelemetryBoundaryRepository,
};
use governance_config::AppConfig;
use governance_domain::{CompletionReason, EvaluationRunState};
use governance_persistence::{
    SeaOrmEvaluationRepository, SeaOrmEvaluationRunRepository, SeaOrmPolicyPackRepository,
};
use sea_orm::Database;
use time::{Duration, OffsetDateTime};

enum JobOutcome {
    Complete,
    Rescheduled,
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();
    let config = AppConfig::from_env()?;
    let poll_milliseconds = u64::try_from(config.telemetry.job_poll_milliseconds).unwrap_or(1_000);
    let lease_seconds = u64::try_from(config.telemetry.job_lease_seconds).unwrap_or(120);
    let max_attempts = u32::try_from(config.telemetry.job_max_attempts).unwrap_or(8);
    let database = Database::connect(&config.database_url).await?;
    let runs = SeaOrmEvaluationRunRepository::new(database.clone());
    let policies = SeaOrmPolicyPackRepository::new(database.clone());
    let evaluations = SeaOrmEvaluationRepository::new(database);
    let mut shutdown = std::pin::pin!(tokio::signal::ctrl_c());
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(poll_milliseconds));

    loop {
        tokio::select! {
            result = &mut shutdown => {
                result?;
                tracing::info!("evaluation worker shutting down");
                break;
            }
            _ = interval.tick() => {
                let Some(job) = runs.claim_due(OffsetDateTime::now_utc(), lease_seconds).await? else {
                    continue;
                };
                let result = match job.kind.as_str() {
                    "evaluation_run_timeout" => {
                        match runs.get_run(job.organization_id, job.eval_run_id).await {
                            Ok(Some(run)) if matches!(
                                run.state,
                                EvaluationRunState::Created | EvaluationRunState::Collecting
                            ) => {
                                CompleteEvaluationRun::new(runs.clone(), runs.clone())
                                    .execute(CompletionRequest {
                                        organization_id: job.organization_id,
                                        eval_run_id: job.eval_run_id,
                                        reason: CompletionReason::MaxDuration,
                                        terminal_state: Some("timed_out".to_owned()),
                                        settle_seconds: 0,
                                    })
                                    .await
                                    .map(|_| JobOutcome::Complete)
                            }
                            Ok(Some(_)) => Ok(JobOutcome::Complete),
                            Ok(None) => Err(ApplicationError::NotFound(job.eval_run_id.to_string())),
                            Err(error) => Err(error),
                        }
                    }
                    "evaluation_run_idle_timeout" => {
                        match runs.get_run(job.organization_id, job.eval_run_id).await {
                            Ok(Some(run)) if matches!(
                                run.state,
                                EvaluationRunState::Created | EvaluationRunState::Collecting
                            ) => {
                                let configured_idle = runs
                                    .get_telemetry_boundary(job.organization_id, &run.target_id)
                                    .await
                                    .map(|boundary| {
                                        boundary
                                            .and_then(|target| target.config.idle_timeout_seconds)
                                    });
                                match configured_idle {
                                    Ok(configured_idle) => {
                                        let idle_seconds = configured_idle
                                            .unwrap_or(u64::try_from(
                                                config.telemetry.default_idle_timeout_seconds,
                                            ).unwrap_or(300))
                                            .clamp(
                                                1,
                                                u64::try_from(
                                                    config.telemetry.max_run_duration_seconds,
                                                ).unwrap_or(86_400),
                                            );
                                        let deadline = run.last_seen_at.unwrap_or(run.created_at)
                                            + Duration::seconds(
                                                i64::try_from(idle_seconds).unwrap_or(i64::MAX),
                                            );
                                        if deadline > OffsetDateTime::now_utc() {
                                            runs.fail_job(
                                                job.id,
                                                "idle deadline advanced",
                                                deadline,
                                                false,
                                            )
                                            .await
                                            .map(|()| JobOutcome::Rescheduled)
                                        } else {
                                            CompleteEvaluationRun::new(
                                                runs.clone(),
                                                runs.clone(),
                                            )
                                            .execute(CompletionRequest {
                                                organization_id: job.organization_id,
                                                eval_run_id: job.eval_run_id,
                                                reason: CompletionReason::IdleTimeout,
                                                terminal_state: Some("timed_out".to_owned()),
                                                settle_seconds: 0,
                                            })
                                            .await
                                            .map(|_| JobOutcome::Complete)
                                        }
                                    }
                                    Err(error) => Err(error),
                                }
                            }
                            Ok(Some(_)) => Ok(JobOutcome::Complete),
                            Ok(None) => Err(ApplicationError::NotFound(job.eval_run_id.to_string())),
                            Err(error) => Err(error),
                        }
                    }
                    "finalize_evaluation_run" => {
                        FinalizeEvaluationRun::new(
                            runs.clone(),
                            runs.clone(),
                            runs.clone(),
                            runs.clone(),
                        )
                        .execute(job.organization_id, job.eval_run_id)
                        .await
                        .map(|_| JobOutcome::Complete)
                    }
                    "evaluate_evidence" => {
                        EvaluateFinalizedRun::new(
                            policies.clone(),
                            runs.clone(),
                            runs.clone(),
                            evaluations.clone(),
                        )
                        .execute(job.organization_id, job.eval_run_id)
                        .await
                        .map(|_| JobOutcome::Complete)
                    }
                    unknown => Err(governance_application::ApplicationError::InvalidRequest(
                        format!("unknown durable job kind {unknown}"),
                    )),
                };
                match result {
                    Ok(JobOutcome::Complete) => {
                        runs.complete_job(job.id, OffsetDateTime::now_utc()).await?;
                    }
                    Ok(JobOutcome::Rescheduled) => {}
                    Err(error) => {
                        let terminal = job.attempts >= max_attempts;
                        let backoff_seconds = 2_i64.pow(job.attempts.min(8));
                        runs.fail_job(
                            job.id,
                            &error.to_string(),
                            OffsetDateTime::now_utc() + Duration::seconds(backoff_seconds),
                            terminal,
                        )
                        .await?;
                        if terminal
                            && let Some(mut run) = runs
                                .get_run(job.organization_id, job.eval_run_id)
                                .await?
                            && !run.state.is_terminal()
                        {
                            run.transition_to(
                                EvaluationRunState::Failed,
                                OffsetDateTime::now_utc(),
                            )
                            .map_err(|transition| anyhow::anyhow!(transition.to_string()))?;
                            runs.update_run(&run).await?;
                        }
                        tracing::warn!(
                            job_id = %job.id,
                            eval_run_id = %job.eval_run_id,
                            attempts = job.attempts,
                            terminal,
                            error = %error,
                            "durable evaluation job failed"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

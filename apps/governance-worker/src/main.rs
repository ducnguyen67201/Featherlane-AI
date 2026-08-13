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
                let job = match runs.claim_due(OffsetDateTime::now_utc(), lease_seconds).await {
                    Ok(job) => job,
                    Err(error) => {
                        tracing::warn!(error = %error, "failed to claim a durable evaluation job");
                        continue;
                    }
                };
                let Some(job) = job else {
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
                                            runs.reschedule_job(
                                                job.id,
                                                job.attempts,
                                                deadline,
                                            )
                                            .await
                                            .map(|failure_recorded| {
                                                if !failure_recorded {
                                                    tracing::warn!(job_id = %job.id, attempts = job.attempts, "job lease was superseded before rescheduling");
                                                }
                                                JobOutcome::Rescheduled
                                            })
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
                        match runs
                            .complete_job(job.id, job.attempts, OffsetDateTime::now_utc())
                            .await
                        {
                            Ok(true) => {}
                            Ok(false) => tracing::warn!(job_id = %job.id, attempts = job.attempts, "job lease was superseded before completion"),
                            Err(error) => tracing::warn!(job_id = %job.id, error = %error, "failed to complete durable job"),
                        }
                    }
                    Ok(JobOutcome::Rescheduled) => {}
                    Err(error) => {
                        let terminal = job.attempts >= max_attempts;
                        let backoff_seconds = 2_i64.pow(job.attempts.min(8));
                        let failure_recorded = match runs
                            .fail_job(
                                job.id,
                                job.attempts,
                                &error.to_string(),
                                OffsetDateTime::now_utc() + Duration::seconds(backoff_seconds),
                                terminal,
                            )
                            .await
                        {
                            Ok(recorded) => recorded,
                            Err(persistence_error) => {
                                tracing::warn!(job_id = %job.id, error = %persistence_error, "failed to persist durable job failure");
                                continue;
                            }
                        };
                        if !failure_recorded {
                            tracing::warn!(job_id = %job.id, attempts = job.attempts, "job lease was superseded before failure handling");
                        }
                        if failure_recorded && terminal {
                            let run = match runs.get_run(job.organization_id, job.eval_run_id).await {
                                Ok(run) => run,
                                Err(persistence_error) => {
                                    tracing::warn!(eval_run_id = %job.eval_run_id, error = %persistence_error, "failed to load run after terminal job failure");
                                    continue;
                                }
                            };
                            if let Some(mut run) = run && !run.state.is_terminal() {
                            let expected_state = run.state;
                            let expected_updated_at = run.updated_at;
                            if let Err(transition) = run.transition_to(
                                EvaluationRunState::Failed,
                                OffsetDateTime::now_utc(),
                            ) {
                                tracing::warn!(eval_run_id = %run.id, error = %transition, "run rejected terminal failure transition");
                            } else {
                                match runs.update_run(&run, expected_state, expected_updated_at).await {
                                    Ok(true) => {}
                                    Ok(false) => tracing::warn!(eval_run_id = %run.id, "run changed while marking a terminal job failure"),
                                    Err(persistence_error) => tracing::warn!(eval_run_id = %run.id, error = %persistence_error, "failed to persist terminal run failure"),
                                }
                            }
                            }
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

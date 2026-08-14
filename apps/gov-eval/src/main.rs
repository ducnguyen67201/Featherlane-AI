use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use governance_domain::{
    EvaluationRun, EvaluationRunState, EvaluationSummary, EvidenceBundle, PolicyPack, RunVerdict,
    ScenarioId,
};
use governance_evaluator::evaluate_pack;
use governance_targets::ScenarioDefinition;
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(name = "gov-eval", about = "Featherlane governance evaluation CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    ListPolicies {
        #[arg(
            long,
            env = "FEATHERLANE_API_URL",
            default_value = "http://127.0.0.1:8080"
        )]
        api_url: String,
    },
    Evaluate {
        #[arg(
            long,
            env = "FEATHERLANE_API_URL",
            default_value = "http://127.0.0.1:8080"
        )]
        api_url: String,
        #[arg(long)]
        policy_pack_id: String,
        #[arg(long)]
        evidence: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
        #[arg(long)]
        fail_on_inconclusive: bool,
    },
    Run {
        #[arg(
            long,
            env = "FEATHERLANE_API_URL",
            default_value = "http://127.0.0.1:8080"
        )]
        api_url: String,
        #[arg(long)]
        target_id: String,
        #[arg(long)]
        policy_pack_id: String,
        #[arg(long)]
        scenario: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
        #[arg(long)]
        fail_on_inconclusive: bool,
    },
    Start {
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        api_url: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        target_version: String,
        #[arg(long)]
        policy_pack_id: String,
        #[arg(long)]
        scenario_id: Option<String>,
        #[arg(long, value_enum, default_value_t = Boundary::ExplicitCi)]
        boundary: Boundary,
        #[arg(long)]
        external_id: Option<String>,
        #[arg(long = "rule")]
        rules: Vec<String>,
        #[arg(long, value_enum, default_value_t = StartFormat::Json)]
        format: StartFormat,
    },
    Complete {
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        api_url: String,
        #[arg(long)]
        run_id: String,
        #[arg(long)]
        terminal_state: Option<String>,
    },
    Wait {
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        api_url: String,
        #[arg(long)]
        run_id: String,
        #[arg(long, default_value_t = 300)]
        timeout: u64,
        #[arg(long, default_value_t = 2)]
        poll_interval: u64,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
        #[arg(long)]
        fail_on_inconclusive: bool,
    },
}

#[derive(Debug, Serialize)]
struct LiveRunRequest<'a> {
    target_id: &'a str,
    policy_pack_id: &'a str,
    scenario: &'a ScenarioDefinition,
}

#[derive(Debug, Deserialize)]
struct LiveRunResponse {
    summary: EvaluationSummary,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Json,
    Junit,
    Html,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum StartFormat {
    Json,
    Shell,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Boundary {
    WorkflowExecution,
    AgentTask,
    VoiceCall,
    ExplicitCi,
}

impl Boundary {
    const fn as_str(self) -> &'static str {
        match self {
            Self::WorkflowExecution => "workflow_execution",
            Self::AgentTask => "agent_task",
            Self::VoiceCall => "voice_call",
            Self::ExplicitCi => "explicit_ci",
        }
    }
}

#[derive(Debug, Deserialize)]
struct RunDetail {
    run: EvaluationRun,
    summary: Option<EvaluationSummary>,
}

#[derive(Debug, Deserialize, Serialize)]
struct StartResponse {
    #[serde(flatten)]
    run: EvaluationRun,
    telemetry: StartTelemetry,
}

#[derive(Debug, Deserialize, Serialize)]
struct StartTelemetry {
    endpoint: String,
    protocol: String,
    attributes: std::collections::BTreeMap<String, String>,
}

#[tokio::main]
async fn main() -> ExitCode {
    match execute(Cli::parse()).await {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(io::stderr(), "error: {error:#}");
            ExitCode::from(2)
        }
    }
}

#[allow(clippy::too_many_lines)]
async fn execute(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::ListPolicies { api_url } => {
            let policies = reqwest::get(format!("{api_url}/v1/policy-packs"))
                .await?
                .error_for_status()?
                .text()
                .await?;
            writeln!(io::stdout(), "{policies}")?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Evaluate {
            api_url,
            policy_pack_id,
            evidence,
            format,
            fail_on_inconclusive,
        } => {
            let pack: PolicyPack =
                reqwest::get(format!("{api_url}/v1/policy-packs/{policy_pack_id}"))
                    .await?
                    .error_for_status()?
                    .json()
                    .await
                    .context("API response was not a persisted policy pack")?;
            pack.ensure_publishable()
                .context("database policy pack is not approved")?;
            let evidence_input = fs::read_to_string(&evidence)
                .with_context(|| format!("failed to read {}", evidence.display()))?;
            let bundle: EvidenceBundle = serde_json::from_str(&evidence_input)
                .context("evidence JSON does not match schema version 1.0")?;
            let summary = evaluate_pack(bundle.eval_run_id, &pack.rules, &bundle);
            let rendered = render_summary(&summary, format)?;
            writeln!(io::stdout(), "{rendered}")?;
            Ok(verdict_exit_code(summary.verdict, fail_on_inconclusive))
        }
        Command::Run {
            api_url,
            target_id,
            policy_pack_id,
            scenario,
            format,
            fail_on_inconclusive,
        } => {
            let scenario_input = fs::read_to_string(&scenario)
                .with_context(|| format!("failed to read {}", scenario.display()))?;
            let scenario_definition: ScenarioDefinition = serde_json::from_str(&scenario_input)
                .with_context(|| {
                    format!(
                        "scenario {} does not match schema version 1.0",
                        scenario.display()
                    )
                })?;
            let response: LiveRunResponse = reqwest::Client::new()
                .post(format!("{api_url}/v1/evaluations"))
                .json(&LiveRunRequest {
                    target_id: &target_id,
                    policy_pack_id: &policy_pack_id,
                    scenario: &scenario_definition,
                })
                .send()
                .await
                .context("live evaluation API request failed")?
                .error_for_status()
                .context("live evaluation API rejected the request")?
                .json()
                .await
                .context("API response was not a live evaluation result")?;
            let rendered = render_summary(&response.summary, format)?;
            writeln!(io::stdout(), "{rendered}")?;
            Ok(verdict_exit_code(
                response.summary.verdict,
                fail_on_inconclusive,
            ))
        }
        Command::Start {
            api_url,
            target,
            target_version,
            policy_pack_id,
            scenario_id,
            boundary,
            external_id,
            rules,
            format,
        } => {
            let scenario_id = scenario_id.unwrap_or_else(|| ScenarioId::new().to_string());
            let response = reqwest::Client::new()
                .post(format!("{api_url}/v1/evaluations"))
                .json(&serde_json::json!({
                    "target_id": target,
                    "target_version": target_version,
                    "policy_pack_id": policy_pack_id,
                    "scenario_id": scenario_id,
                    "boundary_kind": boundary.as_str(),
                    "external_run_id": external_id,
                    "rule_ids": rules,
                }))
                .send()
                .await?
                .error_for_status()?;
            let response: StartResponse = response
                .json()
                .await
                .context("API response was not an evaluation run start response")?;
            match format {
                StartFormat::Json => {
                    writeln!(io::stdout(), "{}", serde_json::to_string(&response)?)?;
                }
                StartFormat::Shell => {
                    writeln!(
                        io::stdout(),
                        "FEATHERLANE_EVAL_RUN_ID='{}'",
                        response.run.id
                    )?;
                    writeln!(
                        io::stdout(),
                        "FEATHERLANE_INVOCATION_ID='{}'",
                        response.run.primary_invocation_id
                    )?;
                    writeln!(
                        io::stdout(),
                        "FEATHERLANE_SCENARIO_ID='{}'",
                        response.run.scenario_id
                    )?;
                    writeln!(
                        io::stdout(),
                        "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT={}",
                        shell_quote(&response.telemetry.endpoint)
                    )?;
                    writeln!(
                        io::stdout(),
                        "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL={}",
                        shell_quote(&response.telemetry.protocol)
                    )?;
                    let correlation = response
                        .telemetry
                        .attributes
                        .iter()
                        .map(|(key, value)| format!("{key}={value}"))
                        .collect::<Vec<_>>()
                        .join(",");
                    let resource_attributes = std::env::var("OTEL_RESOURCE_ATTRIBUTES")
                        .ok()
                        .filter(|value| !value.is_empty())
                        .map_or(correlation.clone(), |existing| {
                            format!("{existing},{correlation}")
                        });
                    writeln!(
                        io::stdout(),
                        "OTEL_RESOURCE_ATTRIBUTES={}",
                        shell_quote(&resource_attributes)
                    )?;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Complete {
            api_url,
            run_id,
            terminal_state,
        } => {
            let run: EvaluationRun = reqwest::Client::new()
                .post(format!("{api_url}/v1/evaluations/{run_id}/complete"))
                .json(&serde_json::json!({
                    "reason": "explicit",
                    "terminal_state": terminal_state,
                }))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await
                .context("API response was not an evaluation run")?;
            writeln!(io::stdout(), "{}", serde_json::to_string(&run)?)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Wait {
            api_url,
            run_id,
            timeout,
            poll_interval,
            format,
            fail_on_inconclusive,
        } => {
            wait_for_run(
                &api_url,
                &run_id,
                timeout,
                poll_interval,
                format,
                fail_on_inconclusive,
            )
            .await
        }
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

async fn wait_for_run(
    api_url: &str,
    run_id: &str,
    timeout_seconds: u64,
    poll_interval_seconds: u64,
    format: OutputFormat,
    fail_on_inconclusive: bool,
) -> Result<ExitCode> {
    let client = reqwest::Client::builder()
        .build()
        .context("failed to build the HTTP client")?;
    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_seconds.max(1));
    let poll_interval = std::time::Duration::from_secs(poll_interval_seconds.max(1));
    let mut last_state = None;
    let mut last_transient_error = None;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            if let Some(error) = last_transient_error {
                anyhow::bail!(
                    "timed out waiting for evaluation run {run_id}; last transient error: {error}"
                );
            }
            anyhow::bail!("timed out waiting for evaluation run {run_id}");
        }
        let request_timeout = deadline
            .saturating_duration_since(now)
            .min(std::time::Duration::from_secs(30));
        let response = client
            .get(format!("{api_url}/v1/evaluations/{run_id}"))
            .timeout(request_timeout)
            .send()
            .await;
        let response = match response {
            Ok(response)
                if response.status().is_server_error()
                    || matches!(
                        response.status(),
                        reqwest::StatusCode::REQUEST_TIMEOUT
                            | reqwest::StatusCode::TOO_MANY_REQUESTS
                    ) =>
            {
                last_transient_error = Some(format!("API returned {}", response.status()));
                sleep_before_retry(deadline, poll_interval).await;
                continue;
            }
            Ok(response) => response.error_for_status()?,
            Err(error) => {
                last_transient_error = Some(error.to_string());
                sleep_before_retry(deadline, poll_interval).await;
                continue;
            }
        };
        last_transient_error = None;
        let detail: RunDetail = response
            .json()
            .await
            .context("API response was not an evaluation run detail")?;
        if last_state != Some(detail.run.state) {
            writeln!(io::stderr(), "state: {:?}", detail.run.state)?;
            last_state = Some(detail.run.state);
        }
        match detail.run.state {
            EvaluationRunState::Completed => {
                let summary = detail
                    .summary
                    .context("completed evaluation did not include a summary")?;
                let rendered = match format {
                    OutputFormat::Json => governance_reports::render_json(&summary)?,
                    OutputFormat::Junit => governance_reports::render_junit(&summary),
                    OutputFormat::Html => governance_reports::render_html(&summary),
                };
                writeln!(io::stdout(), "{rendered}")?;
                return Ok(match summary.verdict {
                    RunVerdict::Fail => ExitCode::from(1),
                    RunVerdict::Inconclusive if fail_on_inconclusive => ExitCode::from(1),
                    RunVerdict::Pass | RunVerdict::Inconclusive => ExitCode::SUCCESS,
                });
            }
            EvaluationRunState::Cancelled | EvaluationRunState::Failed => {
                anyhow::bail!(
                    "evaluation ended in operational state {:?}",
                    detail.run.state
                );
            }
            EvaluationRunState::Created
            | EvaluationRunState::Collecting
            | EvaluationRunState::Settling
            | EvaluationRunState::Finalizing
            | EvaluationRunState::Evaluating => {}
        }
        sleep_before_retry(deadline, poll_interval).await;
    }
}

async fn sleep_before_retry(deadline: tokio::time::Instant, requested: std::time::Duration) {
    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
    if !remaining.is_zero() {
        tokio::time::sleep(requested.min(remaining)).await;
    }
}

fn render_summary(summary: &EvaluationSummary, format: OutputFormat) -> Result<String> {
    Ok(match format {
        OutputFormat::Json => governance_reports::render_json(summary)?,
        OutputFormat::Junit => governance_reports::render_junit(summary),
        OutputFormat::Html => governance_reports::render_html(summary),
    })
}

fn verdict_exit_code(verdict: RunVerdict, fail_on_inconclusive: bool) -> ExitCode {
    match verdict {
        RunVerdict::Fail => ExitCode::from(1),
        RunVerdict::Inconclusive if fail_on_inconclusive => ExitCode::from(1),
        RunVerdict::Pass | RunVerdict::Inconclusive => ExitCode::SUCCESS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_exit_mapping_is_shared_by_all_commands() {
        assert_eq!(verdict_exit_code(RunVerdict::Pass, true), ExitCode::SUCCESS);
        assert_eq!(
            verdict_exit_code(RunVerdict::Fail, false),
            ExitCode::from(1)
        );
        assert_eq!(
            verdict_exit_code(RunVerdict::Inconclusive, false),
            ExitCode::SUCCESS
        );
        assert_eq!(
            verdict_exit_code(RunVerdict::Inconclusive, true),
            ExitCode::from(1)
        );
    }
}

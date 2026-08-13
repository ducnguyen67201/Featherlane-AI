use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use governance_domain::{EvaluationSummary, EvidenceBundle, PolicyPack, RunVerdict};
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

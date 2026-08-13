//! Machine-readable and reviewer-facing evidence report rendering.

use std::fmt::Write as _;

use governance_domain::{EvaluationSummary, RuleStatus, RunVerdict};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReportError {
    #[error("JSON rendering failed: {0}")]
    Json(#[from] serde_json::Error),
}

/// Renders the complete evaluation summary as formatted JSON.
///
/// # Errors
///
/// Returns an error if the summary cannot be serialized.
pub fn render_json(summary: &EvaluationSummary) -> Result<String, ReportError> {
    Ok(serde_json::to_string_pretty(summary)?)
}

pub fn render_junit(summary: &EvaluationSummary) -> String {
    let failures = summary.failed;
    let skipped = summary.inconclusive;
    let mut cases = String::new();
    for result in &summary.results {
        let detail = match result.status {
            RuleStatus::Fail => {
                format!("<failure message=\"{}\" />", escape_xml(&result.message))
            }
            RuleStatus::Uncertain | RuleStatus::NotObservable | RuleStatus::Error => {
                format!("<skipped message=\"{}\" />", escape_xml(&result.message))
            }
            RuleStatus::Pass => String::new(),
        };
        let _ = write!(
            cases,
            "<testcase classname=\"governance\" name=\"{}\">{detail}</testcase>",
            escape_xml(&result.rule_id)
        );
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><testsuite name=\"featherlane\" tests=\"{}\" failures=\"{failures}\" skipped=\"{skipped}\">{cases}</testsuite>",
        summary.results.len()
    )
}

pub fn render_html(summary: &EvaluationSummary) -> String {
    let verdict = match summary.verdict {
        RunVerdict::Pass => "PASS",
        RunVerdict::Fail => "FAIL",
        RunVerdict::Inconclusive => "INCONCLUSIVE",
    };
    let mut rows = String::new();
    for result in &summary.results {
        let _ = write!(
            rows,
            "<tr><td>{}</td><td>{:?}</td><td>{:?}</td><td>{}</td></tr>",
            escape_xml(&result.rule_id),
            result.severity,
            result.status,
            escape_xml(&result.message)
        );
    }
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Featherlane evidence report</title><style>body{{font-family:system-ui;background:#070b0c;color:#eef2ed;padding:32px}}table{{border-collapse:collapse;width:100%}}td,th{{border-bottom:1px solid #26302d;padding:12px;text-align:left}}.notice{{color:#9aa8a3}}</style></head><body><h1>{verdict}</h1><p>Run {}</p><p class=\"notice\">This report contains policy-conformance evidence. It is not a legal certification.</p><table><thead><tr><th>Rule</th><th>Severity</th><th>Status</th><th>Evidence</th></tr></thead><tbody>{rows}</tbody></table></body></html>",
        summary.eval_run_id
    )
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use governance_domain::{EvalRunId, EvaluationSummary, RunVerdict};

    use super::*;

    #[test]
    fn report_contains_non_certification_language() {
        let summary = EvaluationSummary {
            eval_run_id: EvalRunId::new(),
            verdict: RunVerdict::Pass,
            results: vec![],
            passed: 0,
            failed: 0,
            inconclusive: 0,
        };
        assert!(render_html(&summary).contains("not a legal certification"));
    }
}

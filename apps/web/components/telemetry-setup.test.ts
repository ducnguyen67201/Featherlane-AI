import { describe, expect, it } from "vitest";
import { serializeTelemetrySetupForm, telemetryEnvironmentSnippet } from "./telemetry-setup";

describe("telemetry setup serialization", () => {
  it("builds an enabled policy-bound session contract", () => {
    const form = new FormData();
    form.set("auto_evaluation", "on");
    form.set("default_policy_pack_id", "policy-42");
    form.set("boundary_kind", "workflow_execution");
    form.set("external_id_attributes", "workflow.id\ngen_ai.conversation.id");
    form.set("terminal_attribute", "workflow.finished");
    form.set("settle_seconds", "8");
    form.set("idle_timeout_seconds", "180");
    form.set("max_duration_seconds", "1200");

    expect(serializeTelemetrySetupForm(form)).toEqual({
      boundary_kind: "workflow_execution",
      external_id_attributes: ["workflow.id", "gen_ai.conversation.id"],
      terminal_attribute: "workflow.finished",
      default_policy_pack_id: "policy-42",
      settle_seconds: 8,
      idle_timeout_seconds: 180,
      max_duration_seconds: 1200,
      conversation_id_is_task_boundary: false,
    });
  });

  it("removes the default policy when automatic evaluation is disabled", () => {
    const form = new FormData();
    form.set("default_policy_pack_id", "policy-42");

    expect(serializeTelemetrySetupForm(form).default_policy_pack_id).toBeNull();
  });
});

describe("OTLP environment snippet", () => {
  it("contains the target-scoped token and configured endpoint", () => {
    expect(telemetryEnvironmentSnippet("flt_secret", "https://otel.example/v1/traces"))
      .toBe([
        "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT=https://otel.example/v1/traces",
        "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL=http/protobuf",
        "OTEL_EXPORTER_OTLP_HEADERS=Authorization=Bearer flt_secret",
      ].join("\n"));
  });
});

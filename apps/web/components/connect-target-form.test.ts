import { describe, expect, it } from "vitest";
import { parseAttributeList, serializeTargetForm } from "./connect-target-form";
import { buildQuickTestScenario } from "./target-actions";

describe("target form serialization", () => {
  it("uses snake-case fields and converts blank optional values to null", () => {
    const form = new FormData();
    form.set("name", " Refund Agent ");
    form.set("key", "refund-agent-staging");
    form.set("version", "git:test");
    form.set("environment", "staging");
    form.set("driver_type", "http_text");
    form.set("endpoint", "http://refund-agent:8091/v1/messages");
    form.set("reset_endpoint", "");
    form.set("auth_secret_ref", "");
    form.set("timeout_seconds", "30");

    expect(serializeTargetForm(form)).toEqual({
      name: "Refund Agent",
      key: "refund-agent-staging",
      version: "git:test",
      environment: "staging",
      driver_type: "http_text",
      endpoint: "http://refund-agent:8091/v1/messages",
      reset_endpoint: null,
      auth_secret_ref: null,
      timeout_seconds: 30,
      otlp_required: false,
      telemetry_boundary: {
        boundary_kind: "workflow_execution",
        external_id_attributes: ["featherlane.external_run.id"],
        terminal_attribute: null,
        default_policy_pack_id: null,
        settle_seconds: 10,
        idle_timeout_seconds: null,
        max_duration_seconds: null,
        conversation_id_is_task_boundary: false,
      },
    });
  });

  it("serializes an approved automatic evaluation boundary", () => {
    const form = new FormData();
    form.set("name", "Trace Agent");
    form.set("key", "trace-agent");
    form.set("version", "git:test");
    form.set("environment", "staging");
    form.set("driver_type", "webhook");
    form.set("endpoint", "http://agent:8091/webhook");
    form.set("timeout_seconds", "45");
    form.set("auto_evaluation", "on");
    form.set("default_policy_pack_id", "policy-1");
    form.set("boundary_kind", "agent_task");
    form.set("external_id_attributes", "agent.session.id, gen_ai.conversation.id\nagent.session.id");
    form.set("terminal_attribute", "agent.session.finished");
    form.set("settle_seconds", "5");
    form.set("idle_timeout_seconds", "120");
    form.set("max_duration_seconds", "900");
    form.set("conversation_id_is_task_boundary", "on");

    expect(serializeTargetForm(form)).toMatchObject({
      otlp_required: true,
      telemetry_boundary: {
        boundary_kind: "agent_task",
        external_id_attributes: ["agent.session.id", "gen_ai.conversation.id"],
        terminal_attribute: "agent.session.finished",
        default_policy_pack_id: "policy-1",
        settle_seconds: 5,
        idle_timeout_seconds: 120,
        max_duration_seconds: 900,
        conversation_id_is_task_boundary: true,
      },
    });
  });

  it("normalizes ordered attribute lists without duplicates", () => {
    expect(parseAttributeList(" workflow.id,trace.session\nworkflow.id ")).toEqual([
      "workflow.id",
      "trace.session",
    ]);
  });
});

describe("target quick-test scenarios", () => {
  it("builds a text event for the HTTP text adapter", () => {
    expect(buildQuickTestScenario("http_text", "hello").events).toEqual([
      { type: "user_text", text: "hello" },
    ]);
  });

  it("builds a JSON event for the webhook adapter", () => {
    expect(buildQuickTestScenario("webhook", '{"ticket":"T-1"}').events).toEqual([
      { type: "webhook", payload: { ticket: "T-1" } },
    ]);
  });

  it("rejects invalid webhook JSON before calling the API", () => {
    expect(() => buildQuickTestScenario("webhook", "not-json")).toThrow(
      "Webhook input must be valid JSON.",
    );
  });
});

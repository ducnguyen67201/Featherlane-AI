import { describe, expect, it } from "vitest";
import { serializeTargetForm } from "./connect-target-form";
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
    });
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

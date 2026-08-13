import { describe, expect, it } from "vitest";
import { serializeTargetForm } from "./connect-target-form";

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

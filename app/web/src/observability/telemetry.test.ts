import { describe, expect, it } from "vitest";
import {
  TELEMETRY_OPT_OUT_KEY,
  bucketFrameMs,
  buildTelemetryBody,
  createTelemetryClient,
  setTelemetryOptOut,
  telemetryDisabledReason,
  type TelemetryBody,
  type TelemetryEvent,
  type TelemetryStorage,
} from "./telemetry";

function memoryStorage(initial: Record<string, string> = {}): TelemetryStorage {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => {
      values.set(key, value);
    },
  };
}

describe("telemetry contract", () => {
  it("does nothing when no endpoint is configured", async () => {
    const calls: unknown[] = [];
    const client = createTelemetryClient({
      fetchImpl: async (...args) => {
        calls.push(args);
      },
    });

    const result = await client.send({
      name: "session_start",
      payload: { backend: "gpu", appVersion: "test" },
    });

    expect(result).toEqual({ status: "disabled", reason: "missing_endpoint" });
    expect(calls).toHaveLength(0);
  });

  it("sends only the allowlisted payload fields to a configured endpoint", async () => {
    let body: TelemetryBody | null = null;
    const client = createTelemetryClient({
      endpoint: "https://telemetry.invalid/collect",
      nowMs: () => 1234,
      fetchImpl: async (_input, init) => {
        body = JSON.parse(init.body) as TelemetryBody;
      },
    });

    const event = {
      name: "crash",
      payload: {
        bucket: "webgpu-init",
        fatal: false,
        stack: "Error: secret raw stack",
        localStorageDump: "{secret:true}",
        sliderValue: 0.42,
      },
    } as unknown as TelemetryEvent;

    const result = await client.send(event);

    expect(result).toEqual({ status: "sent" });
    expect(body).toEqual({
      schemaVersion: 1,
      event: "crash",
      sentAtMs: 1234,
      payload: { bucket: "webgpu-init", fatal: false },
    });
  });

  it("honors query, local opt-out, and browser privacy signals", () => {
    const endpoint = "https://telemetry.invalid/collect";

    expect(telemetryDisabledReason({ endpoint, locationSearch: "?telemetry=0" })).toBe(
      "query_opt_out",
    );
    expect(
      telemetryDisabledReason({
        endpoint,
        storage: memoryStorage({ [TELEMETRY_OPT_OUT_KEY]: "1" }),
      }),
    ).toBe("local_opt_out");
    expect(
      telemetryDisabledReason({
        endpoint,
        navigatorSignals: { globalPrivacyControl: true },
      }),
    ).toBe("global_privacy_control");
    expect(
      telemetryDisabledReason({
        endpoint,
        navigatorSignals: { doNotTrack: "1" },
      }),
    ).toBe("do_not_track");
  });

  it("stores standalone opt-out without cookies or identity", () => {
    const storage = memoryStorage();

    setTelemetryOptOut(storage, true);
    expect(storage.getItem(TELEMETRY_OPT_OUT_KEY)).toBe("1");

    setTelemetryOptOut(storage, false);
    expect(storage.getItem(TELEMETRY_OPT_OUT_KEY)).toBe("0");
  });

  it("buckets frame-time values for low-cadence runtime performance events", () => {
    expect(bucketFrameMs(4)).toBe("0-8ms");
    expect(bucketFrameMs(12)).toBe("8-16ms");
    expect(bucketFrameMs(24)).toBe("16-33ms");
    expect(bucketFrameMs(40)).toBe("33-50ms");
    expect(bucketFrameMs(75)).toBe("50-100ms");
    expect(bucketFrameMs(140)).toBe("100ms+");
  });

  it("keeps runtime performance payloads bounded to aggregate values", () => {
    const event = {
      name: "runtime_perf",
      payload: {
        fps: 58.2,
        frameAvgMs: 14.1,
        frameP95Ms: 22.8,
        frameP95Bucket: bucketFrameMs(22.8),
        rawFrameTimes: [12, 14, 90],
      },
    } as unknown as TelemetryEvent;

    expect(buildTelemetryBody(event, 5000)).toEqual({
      schemaVersion: 1,
      event: "runtime_perf",
      sentAtMs: 5000,
      payload: {
        fps: 58.2,
        frameAvgMs: 14.1,
        frameP95Ms: 22.8,
        frameP95Bucket: "16-33ms",
      },
    });
  });
});

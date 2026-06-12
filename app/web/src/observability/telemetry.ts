export const TELEMETRY_OPT_OUT_KEY = "bv_telemetry_opt_out";

export type TelemetryEventName =
  | "session_start"
  | "webgpu_init"
  | "startup_timing"
  | "runtime_perf"
  | "crash";

export type TelemetryDisabledReason =
  | "missing_endpoint"
  | "query_opt_out"
  | "local_opt_out"
  | "global_privacy_control"
  | "do_not_track";

export type TelemetrySendResult =
  | { status: "sent" }
  | { status: "disabled"; reason: TelemetryDisabledReason };

export interface SessionStartEvent {
  name: "session_start";
  payload: {
    appVersion?: string;
    backend: "gpu" | "cpu";
  };
}

export interface WebGpuInitEvent {
  name: "webgpu_init";
  payload: {
    gpuPresent: boolean;
    adapterAvailable: boolean;
    adapterClass?: "discrete" | "integrated" | "cpu" | "unknown";
    errorBucket?: string;
  };
}

export interface StartupTimingEvent {
  name: "startup_timing";
  payload: {
    status: "ready" | "failed";
    totalMs: number;
    stageCount: number;
    slowestStageMs?: number;
  };
}

export interface RuntimePerfEvent {
  name: "runtime_perf";
  payload: {
    fps: number;
    frameAvgMs: number;
    frameP95Ms: number;
    frameP95Bucket: FrameTimeBucket;
  };
}

export interface CrashEvent {
  name: "crash";
  payload: {
    bucket: string;
    fatal: boolean;
  };
}

export type TelemetryEvent =
  | SessionStartEvent
  | WebGpuInitEvent
  | StartupTimingEvent
  | RuntimePerfEvent
  | CrashEvent;

export type FrameTimeBucket =
  | "0-8ms"
  | "8-16ms"
  | "16-33ms"
  | "33-50ms"
  | "50-100ms"
  | "100ms+";

export interface TelemetryBody {
  schemaVersion: 1;
  event: TelemetryEventName;
  sentAtMs: number;
  payload: Record<string, boolean | number | string>;
}

export interface TelemetryStorage {
  getItem(key: string): string | null;
  setItem?(key: string, value: string): void;
}

export interface TelemetryNavigatorSignals {
  doNotTrack?: string | null;
  globalPrivacyControl?: boolean;
}

export interface TelemetryEnvironment {
  endpoint?: string | null;
  locationSearch?: string;
  storage?: TelemetryStorage;
  navigatorSignals?: TelemetryNavigatorSignals;
  nowMs?: () => number;
  fetchImpl?: (
    input: string,
    init: {
      method: "POST";
      headers: Record<string, string>;
      body: string;
      keepalive: boolean;
    },
  ) => Promise<unknown>;
}

export function setTelemetryOptOut(
  storage: TelemetryStorage,
  optedOut: boolean,
): void {
  storage.setItem?.(TELEMETRY_OPT_OUT_KEY, optedOut ? "1" : "0");
}

export function telemetryDisabledReason(
  env: TelemetryEnvironment,
): TelemetryDisabledReason | null {
  if (!env.endpoint) return "missing_endpoint";
  const params = new URLSearchParams((env.locationSearch ?? "").replace(/^\?/, ""));
  if (params.get("telemetry") === "0") return "query_opt_out";
  if (env.storage?.getItem(TELEMETRY_OPT_OUT_KEY) === "1") return "local_opt_out";
  if (env.navigatorSignals?.globalPrivacyControl === true) {
    return "global_privacy_control";
  }
  const dnt = env.navigatorSignals?.doNotTrack?.toLowerCase();
  if (dnt === "1" || dnt === "yes") return "do_not_track";
  return null;
}

export function bucketFrameMs(frameMs: number): FrameTimeBucket {
  if (frameMs < 8) return "0-8ms";
  if (frameMs < 16) return "8-16ms";
  if (frameMs < 33) return "16-33ms";
  if (frameMs < 50) return "33-50ms";
  if (frameMs < 100) return "50-100ms";
  return "100ms+";
}

export function buildTelemetryBody(
  event: TelemetryEvent,
  sentAtMs: number,
): TelemetryBody {
  return {
    schemaVersion: 1,
    event: event.name,
    sentAtMs,
    payload: sanitizePayload(event),
  };
}

export function createTelemetryClient(env: TelemetryEnvironment) {
  return {
    async send(event: TelemetryEvent): Promise<TelemetrySendResult> {
      const reason = telemetryDisabledReason(env);
      if (reason) return { status: "disabled", reason };
      const fetchImpl = env.fetchImpl ?? fetch;
      const endpoint = env.endpoint as string;
      await fetchImpl(endpoint, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(buildTelemetryBody(event, env.nowMs?.() ?? Date.now())),
        keepalive: true,
      });
      return { status: "sent" };
    },
  };
}

function sanitizePayload(event: TelemetryEvent): Record<string, boolean | number | string> {
  switch (event.name) {
    case "session_start":
      return copyAllowed(event.payload, ["appVersion", "backend"]);
    case "webgpu_init":
      return copyAllowed(event.payload, [
        "gpuPresent",
        "adapterAvailable",
        "adapterClass",
        "errorBucket",
      ]);
    case "startup_timing":
      return copyAllowed(event.payload, [
        "status",
        "totalMs",
        "stageCount",
        "slowestStageMs",
      ]);
    case "runtime_perf":
      return copyAllowed(event.payload, [
        "fps",
        "frameAvgMs",
        "frameP95Ms",
        "frameP95Bucket",
      ]);
    case "crash":
      return copyAllowed(event.payload, ["bucket", "fatal"]);
  }
}

function copyAllowed(
  source: Record<string, unknown>,
  keys: readonly string[],
): Record<string, boolean | number | string> {
  const payload: Record<string, boolean | number | string> = {};
  for (const key of keys) {
    const value = source[key];
    if (
      typeof value === "boolean" ||
      typeof value === "number" ||
      typeof value === "string"
    ) {
      payload[key] = value;
    }
  }
  return payload;
}

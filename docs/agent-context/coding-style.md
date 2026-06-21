# Coding style

## When does this apply

You're editing Rust, WGSL, or TypeScript in this repo. The generic style rules
are in the global kit
([`~/.agentdocs/rules/coding-style.md`](~/.agentdocs/rules/coding-style.md));
below are the app-specific ones that the global rules don't cover.

## App-specific rules

- **No per-frame allocation in hot paths.** The rAF loop and the GPU tick path
  must not rebuild `Vec`s, JS arrays, bind groups, pipelines, or render targets.
  Allocation is allowed only on structural change (tier resize, backend restart,
  device loss, render-target resize). See
  [`../architecture/gpu-backend.md`](../architecture/gpu-backend.md).
- **No CPU↔GPU readback in the rAF loop.** Readbacks go through the async
  Idle/Pending staging path. See
  [`../architecture/profiling.md`](../architecture/profiling.md).
- **Rust/WGSL determinism is a contract.** Any change to the hash primitive or the
  `target`/`weight` rule must keep Rust and WGSL byte-identical and pass the
  determinism gates ([`testing-how-to.md`](testing-how-to.md)). The hash and
  rule are owned by [`../architecture/connectivity.md`](../architecture/connectivity.md).
- **Shared struct layouts are corruption risks.** Any `#[repr]` struct that is
  also declared in WGSL (`MorphSegment`, edge events) or any `Float32Array`
  settings/metrics index contract must change on both sides together. The owning
  docs flag each one.
- **Keep modules separated.** sim, render, controls, profiler, dev panel, and
  GPU resource lifecycle stay in their own modules; `web/src/main.ts` orchestrates
  but does not own all state.
- **Debug/HUD code stays behind flags.** Production passes must not depend on
  debug buffers; debug passes may read production buffers.

## See also

- [`~/.agentdocs/rules/coding-style.md`](~/.agentdocs/rules/coding-style.md) — generic rules.
- [`repo-rules.md`](repo-rules.md), [`testing-how-to.md`](testing-how-to.md).
- [`index.md`](index.md) — agent-context router.

#![cfg(not(target_arch = "wasm32"))]

pub fn strict_wgpu_tests_enabled() -> bool {
    env_enabled("BV_REQUIRE_WGPU_TESTS") || env_enabled("CI")
}

fn env_enabled(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            !v.is_empty() && v != "0" && v != "false" && v != "no"
        })
        .unwrap_or(false)
}

fn skip_or_panic(test_name: &str, err: impl std::fmt::Display) {
    if strict_wgpu_tests_enabled() {
        panic!("{test_name}: required wgpu adapter/device unavailable: {err}");
    }
    eprintln!("SKIP {test_name}: no wgpu adapter/device ({err})");
}

#[allow(dead_code)]
pub async fn request_native_adapter_or_skip(
    test_name: &str,
    instance: &wgpu::Instance,
) -> Option<wgpu::Adapter> {
    match instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
    {
        Ok(adapter) => Some(adapter),
        Err(e) => {
            skip_or_panic(test_name, e);
            None
        }
    }
}

#[allow(dead_code)]
pub async fn acquire_native_context_or_skip(
    test_name: &str,
) -> Option<brain_visualizer::sim::gpu::GpuContext> {
    match brain_visualizer::sim::gpu::GpuBackend::acquire_native().await {
        Ok(ctx) => Some(ctx),
        Err(e) => {
            skip_or_panic(test_name, e);
            None
        }
    }
}

param(
    [switch]$SkipDockerBuild,
    [switch]$SkipTests
)

$ErrorActionPreference = "Stop"

if (-not $SkipDockerBuild) {
    docker build --target test -t trident:test .
}

cargo check --benches --tests
cargo clippy --all-targets -- -D warnings

if (-not $SkipTests) {
    cargo test --all
}

$gpuFeatures = @(
    "gpu-cuda",
    "gpu-vulkan",
    "gpu-metal",
    "gpu-wgpu",
    "gpu-cuda,gpu-vulkan,gpu-metal,gpu-wgpu"
)

foreach ($features in $gpuFeatures) {
    cargo check --benches --tests --features $features
}

docker run --rm -v "${PWD}:/workspace" -w /workspace trident:test sh -lc "/usr/local/cargo/bin/cargo check --benches --tests && /usr/local/cargo/bin/cargo check --benches --tests --features gpu-cuda,gpu-vulkan,gpu-metal,gpu-wgpu && /usr/local/cargo/bin/cargo test --all"

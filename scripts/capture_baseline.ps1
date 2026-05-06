param(
    [string]$OutputRoot = "docs/benchmarks/baselines",
    [switch]$IncludeSoak,
    [switch]$SkipStress
)

$ErrorActionPreference = "Stop"

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$outDir = Join-Path $OutputRoot $timestamp
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$metadata = [ordered]@{
    captured_at = (Get-Date).ToUniversalTime().ToString("o")
    machine = $env:COMPUTERNAME
    os = (Get-CimInstance Win32_OperatingSystem).Caption
    cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty Name)
    logical_processors = (Get-CimInstance Win32_Processor | Measure-Object -Property NumberOfLogicalProcessors -Sum).Sum
    total_memory_bytes = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    rustc = (& rustc --version)
    cargo = (& cargo --version)
    git_commit = (& git rev-parse HEAD)
}

$metadata | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 (Join-Path $outDir "hardware.json")

if (-not $SkipStress) {
    cargo test --test concurrency_stress -- --ignored 2>&1 |
        Tee-Object -FilePath (Join-Path $outDir "stress-tests.log")
}

cargo bench --bench kv_storage 2>&1 |
    Tee-Object -FilePath (Join-Path $outDir "kv-storage.log")

cargo bench --bench concurrency_mixed_workload 2>&1 |
    Tee-Object -FilePath (Join-Path $outDir "concurrency-mixed-workload.log")

cargo bench --bench accel_cpu_crc 2>&1 |
    Tee-Object -FilePath (Join-Path $outDir "accel-cpu-crc.log")

cargo bench --bench accel_gpu_crc 2>&1 |
    Tee-Object -FilePath (Join-Path $outDir "accel-gpu-crc.log")

if ($IncludeSoak) {
    $env:TRIDENT_INCLUDE_SOAK_BENCH = "1"
    cargo bench --bench concurrency_scaling 2>&1 |
        Tee-Object -FilePath (Join-Path $outDir "concurrency-scaling.log")
    Remove-Item Env:\TRIDENT_INCLUDE_SOAK_BENCH
} else {
    cargo bench --bench concurrency_scaling 2>&1 |
        Tee-Object -FilePath (Join-Path $outDir "concurrency-scaling.log")
}

"Baseline capture written to $outDir" | Tee-Object -FilePath (Join-Path $outDir "summary.txt")

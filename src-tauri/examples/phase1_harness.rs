use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
use voicewave_core_lib::phase1::{benchmark_models, run_stability_sessions, StabilitySummary};

#[derive(Debug)]
enum HarnessMode {
    Benchmark,
    Stability,
    Sustained,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("phase1 harness failed: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let mode = parse_mode(args.get(1).map(String::as_str).unwrap_or("benchmark"))?;
    let output_path = parse_arg_value(&args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_output_path(&mode));

    match mode {
        HarnessMode::Benchmark => {
            let runs = parse_arg_value(&args, "--runs")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(20);
            let models = vec!["tiny.en", "base.en", "small.en", "medium.en"];
            let report = benchmark_models(&models, runs, None).await?;
            write_json_report(&output_path, &report)?;
            println!("benchmark report written to {}", output_path.display());
        }
        HarnessMode::Stability => {
            let sessions = parse_arg_value(&args, "--sessions")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(200);
            let summary = run_stability_sessions("small.en", sessions, Some(1)).await;
            write_json_report(&output_path, &summary)?;
            println!("stability report written to {}", output_path.display());
        }
        HarnessMode::Sustained => {
            let minutes = parse_arg_value(&args, "--minutes")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(30);
            let report = run_sustained_workload(minutes).await;
            write_json_report(&output_path, &report)?;
            println!("sustained report written to {}", output_path.display());
        }
    }

    Ok(())
}

fn parse_mode(raw: &str) -> Result<HarnessMode, String> {
    match raw {
        "benchmark" => Ok(HarnessMode::Benchmark),
        "stability" => Ok(HarnessMode::Stability),
        "sustained" => Ok(HarnessMode::Sustained),
        _ => Err(format!(
            "unknown mode '{raw}', expected benchmark|stability|sustained"
        )),
    }
}

fn parse_arg_value<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == key)
        .and_then(|idx| args.get(idx + 1))
        .map(String::as_str)
}

fn default_output_path(mode: &HarnessMode) -> PathBuf {
    let filename = match mode {
        HarnessMode::Benchmark => "phase1-latency-baseline.json",
        HarnessMode::Stability => "phase1-stability-200.json",
        HarnessMode::Sustained => "phase1-sustained-30m.json",
    };
    PathBuf::from("../docs/phase1").join(filename)
}

fn write_json_report<T: serde::Serialize>(path: &PathBuf, report: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("failed creating output dir: {err}"))?;
    }
    let data = serde_json::to_string_pretty(report)
        .map_err(|err| format!("failed serializing report: {err}"))?;
    fs::write(path, data).map_err(|err| format!("failed writing report: {err}"))?;
    Ok(())
}

async fn run_sustained_workload(minutes: u64) -> StabilitySummary {
    let target_duration = Duration::from_secs(minutes.saturating_mul(60));
    let started = Instant::now();
    let mut sessions = 0usize;
    let mut failures = 0usize;
    let mut latencies = Vec::new();

    while started.elapsed() < target_duration {
        sessions += 1;
        let summary = run_stability_sessions("small.en", 1, Some(1)).await;
        failures += summary.failed_sessions;
        if summary.failed_sessions == 0 {
            latencies.push(summary.p95_latency_ms);
        }
    }

    latencies.sort_unstable();
    let crash_free_rate = if sessions == 0 {
        1.0
    } else {
        (sessions.saturating_sub(failures)) as f32 / sessions as f32
    };
    let p95_latency_ms = if latencies.is_empty() {
        0
    } else {
        let idx = ((latencies.len() as f32 - 1.0) * 0.95).round() as usize;
        latencies[idx.min(latencies.len() - 1)]
    };

    StabilitySummary {
        sessions,
        failed_sessions: failures,
        crash_free_rate,
        p95_latency_ms,
    }
}

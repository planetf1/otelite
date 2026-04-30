use std::process::Command;

fn main() {
    // Allow CI to inject the full SHA via env var (avoids requiring fetch-depth: 0)
    let sha = std::env::var("OTELITE_GIT_SHA")
        .ok()
        .map(|s| s.chars().take(7).collect::<String>())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .ok()
                .and_then(|out| {
                    if out.status.success() {
                        String::from_utf8(out.stdout)
                            .ok()
                            .map(|s| s.trim().to_string())
                    } else {
                        None
                    }
                })
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=OTELITE_GIT_SHA={sha}");

    // Emit build date in YYYY-MM-DD format
    let date = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|epoch| epoch.parse::<i64>().ok())
        .map(|ts| {
            let secs = ts;
            // Days since Unix epoch
            let days = secs / 86400;
            // Simple Gregorian calendar calculation
            epoch_days_to_date(days)
        })
        .unwrap_or_else(|| {
            // Fall back to current system date
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let days = now / 86400;
            epoch_days_to_date(days)
        });

    println!("cargo:rustc-env=OTELITE_BUILD_DATE={date}");

    // Re-run if HEAD changes
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads");
}

/// Convert days since Unix epoch (1970-01-01) to YYYY-MM-DD string.
fn epoch_days_to_date(days: i64) -> String {
    // Algorithm: http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

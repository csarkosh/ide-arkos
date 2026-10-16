//! Records the short git hash so `--version` names the exact build (update-in-place spec,
//! section 6). `unknown` outside a git checkout. Also records the build date.
fn main() {
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=ARKOS_GIT_HASH={hash}");
    println!("cargo:rustc-env=ARKOS_BUILD_DATE={}", build_date());
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");
}

/// `SOURCE_DATE_EPOCH` as `YYYY-MM-DD` when set (a reproducible build), else the commit date from
/// git (which matches the hash), else `unknown`.
fn build_date() -> String {
    if let Some(secs) = std::env::var("SOURCE_DATE_EPOCH").ok().and_then(|v| v.trim().parse::<i64>().ok()) {
        return civil_date(secs.div_euclid(86_400));
    }
    std::process::Command::new("git")
        .args(["log", "-1", "--format=%cs"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Days since 1970-01-01 as `YYYY-MM-DD` (Howard Hinnant's civil-from-days algorithm).
fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

//! When the running program was built, and from which branch and commit,
//! as `build.rs` recorded them. The About window of a debug build shows it,
//! so a test copy says which build it is.

/// One line for the About window, such as "Built 2026-11-10 23:41 EST,
/// dark-mode d3f57d0", in the building machine's time when it was known.
/// "modified" at the end means the build had changes that were not
/// committed.
pub fn summary() -> String {
    let time = option_env!("QUILL_BUILD_LOCAL_TIME")
        .map(str::to_string)
        .unwrap_or_else(|| utc(env!("QUILL_BUILD_TIME").parse().unwrap_or(0)));
    describe(
        &time,
        option_env!("QUILL_BUILD_BRANCH"),
        option_env!("QUILL_BUILD_COMMIT"),
        option_env!("QUILL_BUILD_DIRTY") == Some("1"),
    )
}

fn describe(time: &str, branch: Option<&str>, commit: Option<&str>, dirty: bool) -> String {
    let mut s = format!("Built {time}");
    match (branch, commit) {
        (Some(b), Some(c)) => s += &format!(", {b} {c}"),
        // A detached HEAD has a commit but no branch.
        (None, Some(c)) => s += &format!(", {c}"),
        _ => {}
    }
    if dirty {
        s += ", modified";
    }
    s
}

/// Seconds since 1970 as "YYYY-MM-DD HH:MM UTC".
fn utc(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = quill_core::revision::civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_as_utc_dates() {
        assert_eq!(utc(0), "1970-01-01 00:00 UTC");
        // 2024-02-29 12:34 UTC, a leap day.
        assert_eq!(utc(1_709_210_040), "2024-02-29 12:34 UTC");
        assert_eq!(utc(1_793_487_660), "2026-10-31 23:01 UTC");
    }

    #[test]
    fn the_line_names_branch_commit_and_uncommitted_changes() {
        assert_eq!(
            describe(&utc(0), Some("main"), Some("d3f57d0"), false),
            "Built 1970-01-01 00:00 UTC, main d3f57d0"
        );
        assert_eq!(
            describe(&utc(0), None, Some("d3f57d0"), true),
            "Built 1970-01-01 00:00 UTC, d3f57d0, modified"
        );
        assert_eq!(
            describe(&utc(0), None, None, false),
            "Built 1970-01-01 00:00 UTC"
        );
        assert!(summary().starts_with("Built "));
    }
}

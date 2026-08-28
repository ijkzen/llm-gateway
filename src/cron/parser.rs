use std::str::FromStr;
use std::time::Duration;

use croner::Cron;

use super::SchedulerError;

#[derive(Clone, Debug)]
pub enum ScheduleType {
    Cron(String),
    Every(Duration),
}

pub fn parse_expression(expr: &str) -> Result<ScheduleType, String> {
    let expr = expr.trim();

    match expr {
        "@yearly" | "@annually" => Ok(ScheduleType::Cron("0 0 0 1 1 *".to_string())),
        "@monthly" => Ok(ScheduleType::Cron("0 0 0 1 * *".to_string())),
        "@weekly" => Ok(ScheduleType::Cron("0 0 0 * * 0".to_string())),
        "@daily" | "@midnight" => Ok(ScheduleType::Cron("0 0 0 * * *".to_string())),
        "@hourly" => Ok(ScheduleType::Cron("0 0 * * * *".to_string())),
        _ => {
            if expr == "@every" {
                return Err("duration cannot be empty".to_string());
            }
            if let Some(dur_str) = expr.strip_prefix("@every ") {
                let dur = parse_duration(dur_str)?;
                Ok(ScheduleType::Every(dur))
            } else {
                let parts: Vec<&str> = expr.split_whitespace().collect();
                match parts.len() {
                    5 => Ok(ScheduleType::Cron(format!("0 {}", expr))),
                    6 => Ok(ScheduleType::Cron(expr.to_string())),
                    _ => Err(format!(
                        "invalid cron expression '{}', expected 5 or 6 fields",
                        expr
                    )),
                }
            }
        }
    }
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("duration cannot be empty".to_string());
    }

    let mut total_secs: u64 = 0;
    let mut i = 0;
    let bytes = s.as_bytes();

    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if start == i {
            return Err(format!("expected number at position {} in '{}'", i, s));
        }
        let num_str = &s[start..i];
        let num: u64 = num_str
            .parse()
            .map_err(|_| format!("invalid number '{}' in '{}'", num_str, s))?;

        if i >= bytes.len() {
            return Err(format!("missing unit after '{}' in '{}'", num_str, s));
        }
        let unit = bytes[i] as char;
        i += 1;

        let multiplier: u64 = match unit {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86400,
            _ => {
                return Err(format!(
                    "unknown unit '{}' in '{}', expected s/m/h/d",
                    unit, s
                ));
            }
        };
        let secs = num
            .checked_mul(multiplier)
            .ok_or_else(|| format!("duration '{}' is too large", s))?;
        total_secs = total_secs
            .checked_add(secs)
            .ok_or_else(|| format!("duration '{}' is too large", s))?;
    }

    if total_secs == 0 {
        return Err(format!("duration '{}' must be greater than zero", s));
    }

    Ok(Duration::from_secs(total_secs))
}

pub fn compute_next_run(
    expression: &str,
) -> Result<chrono::DateTime<chrono::Utc>, SchedulerError> {
    compute_next_run_from_scheduled_at(expression, chrono::Utc::now())
}

pub fn compute_next_run_from_scheduled_at(
    expression: &str,
    scheduled_at: chrono::DateTime<chrono::Utc>,
) -> Result<chrono::DateTime<chrono::Utc>, SchedulerError> {
    let schedule = parse_expression(expression).map_err(SchedulerError::ParseError)?;
    match schedule {
        ScheduleType::Cron(cron_expr) => {
            let cron = Cron::from_str(&cron_expr)
                .map_err(|e| SchedulerError::ParseError(e.to_string()))?;
            // Cron expressions are interpreted in the server's local timezone:
            // "0 0 8 * * *" means 08:00 local time, matching user expectation
            // and the local-time display in the frontend.
            let local = scheduled_at.with_timezone(&chrono::Local);
            let next = cron
                .find_next_occurrence(&local, false)
                .map_err(|e| SchedulerError::ComputeNextRun(e.to_string()))?;
            Ok(next.with_timezone(&chrono::Utc))
        }
        ScheduleType::Every(duration) => {
            let secs = i64::try_from(duration.as_secs()).map_err(|_| {
                SchedulerError::ComputeNextRun(format!("duration '{}' is too large", expression))
            })?;
            let delta = chrono::TimeDelta::try_seconds(secs).ok_or_else(|| {
                SchedulerError::ComputeNextRun(format!("duration '{}' is too large", expression))
            })?;
            scheduled_at.checked_add_signed(delta).ok_or_else(|| {
                SchedulerError::ComputeNextRun(format!(
                    "duration '{}' overflows next run time",
                    expression
                ))
            })
        }
    }
}

pub fn compute_frequency_secs(expression: &str) -> i64 {
    match parse_expression(expression) {
        Ok(ScheduleType::Every(duration)) => i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        Ok(ScheduleType::Cron(cron_expr)) => estimate_cron_period(&cron_expr),
        Err(_) => i64::MAX,
    }
}

/// Estimates the period of a cron expression by computing its next three
/// occurrences with croner and averaging the intervals between them. This is
/// exact for regular expressions (hourly, daily, weekly, `*/n`) and a close
/// approximation for irregular ones (e.g. monthly intervals vary between 28
/// and 31 days). Returns i64::MAX when fewer than two future occurrences can
/// be computed.
fn estimate_cron_period(cron_expr: &str) -> i64 {
    let Ok(cron) = Cron::from_str(cron_expr) else {
        return i64::MAX;
    };

    let mut cursor = chrono::Utc::now().with_timezone(&chrono::Local);
    let mut occurrences = Vec::with_capacity(3);
    for _ in 0..3 {
        match cron.find_next_occurrence(&cursor, false) {
            Ok(next) => {
                occurrences.push(next);
                cursor = next;
            }
            Err(_) => break,
        }
    }

    if occurrences.len() < 2 {
        return i64::MAX;
    }

    let total: i64 = occurrences
        .windows(2)
        .map(|w| (w[1] - w[0]).num_seconds())
        .sum();
    total / (occurrences.len() - 1) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_frequency_secs_every() {
        assert_eq!(compute_frequency_secs("@every 5m"), 300);
        assert_eq!(compute_frequency_secs("@every 1h"), 3600);
        assert_eq!(compute_frequency_secs("@every 24h"), 86400);
        assert_eq!(compute_frequency_secs("@every 90m"), 5400);
    }

    #[test]
    fn test_compute_frequency_secs_shorthands() {
        assert_eq!(compute_frequency_secs("@hourly"), 3600);
        assert_eq!(compute_frequency_secs("@daily"), 86400);
        assert_eq!(compute_frequency_secs("@weekly"), 604800);
        // Monthly intervals vary between 28 and 31 days.
        let monthly = compute_frequency_secs("@monthly");
        assert!(
            (28 * 86400..=31 * 86400).contains(&monthly),
            "monthly estimate out of range: {monthly}"
        );
    }

    #[test]
    fn test_compute_frequency_secs_cron() {
        assert_eq!(compute_frequency_secs("0 0 */6 * * *"), 6 * 3600);
        assert_eq!(compute_frequency_secs("0 */10 * * * *"), 600);
        assert_eq!(compute_frequency_secs("0 0 * * * *"), 3600);
    }

    #[test]
    fn test_compute_frequency_secs_monthly_and_weekly_cron() {
        // Regression: day-of-month / day-of-week constrained expressions used
        // to be estimated as 1 day, off by 7x-30x.
        let monthly = compute_frequency_secs("0 0 0 15 * *");
        assert!(
            (28 * 86400..=31 * 86400).contains(&monthly),
            "monthly-on-15th estimate out of range: {monthly}"
        );
        assert_eq!(compute_frequency_secs("0 0 0 * * 1"), 604800);
    }

    #[test]
    fn test_compute_next_run_cron_uses_local_timezone() {
        use chrono::Timelike;

        // "0 0 8 * * *" must mean 08:00 in the server's local timezone,
        // regardless of which timezone the test runs in.
        let base = chrono::DateTime::parse_from_rfc3339("2026-01-15T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let next = compute_next_run_from_scheduled_at("0 0 8 * * *", base).unwrap();
        let local = next.with_timezone(&chrono::Local);
        assert_eq!(local.hour(), 8);
        assert_eq!(local.minute(), 0);
        assert!(next > base);
    }

    #[test]
    fn test_compute_frequency_secs_invalid() {
        assert_eq!(compute_frequency_secs("invalid"), i64::MAX);
    }

    #[test]
    fn test_parse_expression_rejects_overflow_duration() {
        // A number that parses as u64 but overflows when multiplied by the day multiplier.
        let result = parse_expression("@every 9999999999999999999d");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("too large"),
            "expected overflow error, got: {}",
            err
        );
    }

    #[test]
    fn test_compute_next_run_rejects_duration_above_i64_max() {
        // u64::MAX seconds is larger than i64::MAX, so compute_next_run should fail.
        let result = compute_next_run("@every 18446744073709551615s");
        assert!(
            matches!(result, Err(SchedulerError::ComputeNextRun(_))),
            "expected ComputeNextRun error, got: {:?}",
            result
        );
    }

    #[test]
    fn test_compute_frequency_secs_every_above_i64_max() {
        // u64::MAX seconds should not wrap to negative; it should saturate to i64::MAX.
        assert_eq!(
            compute_frequency_secs("@every 18446744073709551615s"),
            i64::MAX
        );
    }

    #[test]
    fn test_parse_expression_rejects_empty_every_duration() {
        let result = parse_expression("@every ");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("cannot be empty"),
            "expected empty duration error, got: {}",
            err
        );
    }

    #[test]
    fn test_parse_expression_rejects_zero_every_duration() {
        assert!(parse_expression("@every 0s").is_err());
        assert!(parse_expression("@every 0m").is_err());
    }

    #[test]
    fn test_parse_expression_accepts_composite_every_duration_with_zero_component() {
        let result = parse_expression("@every 1h0m");
        assert!(result.is_ok());
    }
}

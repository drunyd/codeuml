//! Configuration for the CRAP overlay, read from `codeuml.toml`.
//!
//! ```toml
//! [crap]
//! good_max = 6      # scores <= good_max are "good" (green)
//! medium_max = 12   # scores <= medium_max are "medium" (yellow); above is "bad" (red)
//! report = ".metrics/crap.edn"
//! ```

use std::path::PathBuf;

use toml::{Table, Value};

use crate::MetricsError;

pub const CONFIG_FILE: &str = "codeuml.toml";
pub const DEFAULT_REPORT: &str = ".metrics/crap.edn";

/// Score limits separating the good / medium / bad bands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrapThresholds {
    pub good_max: f64,
    pub medium_max: f64,
}

impl Default for CrapThresholds {
    fn default() -> Self {
        Self {
            good_max: 6.0,
            medium_max: 12.0,
        }
    }
}

/// How a score compares with the configured thresholds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrapLevel {
    Good,
    Medium,
    Bad,
}

impl CrapThresholds {
    pub fn level(&self, score: f64) -> CrapLevel {
        if score <= self.good_max {
            CrapLevel::Good
        } else if score <= self.medium_max {
            CrapLevel::Medium
        } else {
            CrapLevel::Bad
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetricsConfig {
    pub thresholds: CrapThresholds,
    /// Report path, relative to the project root.
    pub report: PathBuf,
    /// Whether the report path came from the config file. A missing default
    /// report is normal; a missing configured one deserves a warning.
    pub report_configured: bool,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            thresholds: CrapThresholds::default(),
            report: PathBuf::from(DEFAULT_REPORT),
            report_configured: false,
        }
    }
}

/// Parse the text of a `codeuml.toml`. Unknown tables and keys are ignored.
pub fn parse_config(text: &str) -> Result<MetricsConfig, MetricsError> {
    let table: Table = text
        .parse()
        .map_err(|error| MetricsError::Config(format!("invalid TOML: {error}")))?;
    let mut config = MetricsConfig::default();
    let Some(crap) = table.get("crap") else {
        return Ok(config);
    };
    let crap = crap
        .as_table()
        .ok_or_else(|| MetricsError::Config("`crap` must be a table".to_owned()))?;

    config.thresholds = read_thresholds(crap)?;
    if let Some(report) = crap.get("report") {
        config.report = read_report_path(report)?;
        config.report_configured = true;
    }
    Ok(config)
}

fn read_thresholds(crap: &Table) -> Result<CrapThresholds, MetricsError> {
    let defaults = CrapThresholds::default();
    let thresholds = CrapThresholds {
        good_max: read_number(crap, "good_max", defaults.good_max)?,
        medium_max: read_number(crap, "medium_max", defaults.medium_max)?,
    };
    if thresholds.good_max > thresholds.medium_max {
        return Err(MetricsError::Config(format!(
            "`good_max` ({}) must not exceed `medium_max` ({})",
            thresholds.good_max, thresholds.medium_max
        )));
    }
    Ok(thresholds)
}

fn read_number(table: &Table, key: &str, default: f64) -> Result<f64, MetricsError> {
    let Some(value) = table.get(key) else {
        return Ok(default);
    };
    let number = match value {
        Value::Float(float) => Some(*float),
        Value::Integer(integer) => Some(*integer as f64),
        _ => None,
    };
    number
        .filter(|number| number.is_finite() && *number >= 0.0)
        .ok_or_else(|| MetricsError::Config(format!("`crap.{key}` must be a non-negative number")))
}

fn read_report_path(value: &Value) -> Result<PathBuf, MetricsError> {
    value
        .as_str()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| MetricsError::Config("`crap.report` must be a non-empty string".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_split_at_six_and_twelve() {
        let thresholds = CrapThresholds::default();
        assert_eq!(thresholds.level(0.0), CrapLevel::Good);
        assert_eq!(thresholds.level(6.0), CrapLevel::Good);
        assert_eq!(thresholds.level(6.01), CrapLevel::Medium);
        assert_eq!(thresholds.level(12.0), CrapLevel::Medium);
        assert_eq!(thresholds.level(12.01), CrapLevel::Bad);
        assert_eq!(parse_config("").expect("empty"), MetricsConfig::default());
        assert_eq!(
            parse_config("[other]\nx = 1").expect("other"),
            MetricsConfig::default()
        );
    }

    #[test]
    fn configured_limits_and_report_are_read() {
        let config = parse_config("[crap]\ngood_max = 4\nmedium_max = 9.5\nreport = \"out/c.edn\"")
            .expect("config");
        assert_eq!(config.thresholds.good_max, 4.0);
        assert_eq!(config.thresholds.medium_max, 9.5);
        assert_eq!(config.report, PathBuf::from("out/c.edn"));
        assert!(config.report_configured);

        let partial = parse_config("[crap]\nmedium_max = 20").expect("partial");
        assert_eq!(partial.thresholds.good_max, 6.0);
        assert_eq!(partial.thresholds.medium_max, 20.0);
        assert!(!partial.report_configured);
    }

    #[test]
    fn invalid_configuration_is_rejected_with_a_reason() {
        for (text, expected) in [
            ("[crap", "invalid TOML"),
            ("crap = 3", "must be a table"),
            ("[crap]\ngood_max = \"six\"", "good_max"),
            ("[crap]\ngood_max = -1", "good_max"),
            ("[crap]\ngood_max = 9\nmedium_max = 3", "must not exceed"),
            ("[crap]\nreport = 5", "crap.report"),
            ("[crap]\nreport = \" \"", "crap.report"),
        ] {
            let error = parse_config(text).expect_err(text);
            assert!(error.to_string().contains(expected), "{text}: {error}");
        }
    }
}

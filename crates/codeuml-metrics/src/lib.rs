//! CRAP-score overlay for CodeUML.
//!
//! Reads a `crapper` report plus the optional `codeuml.toml` and answers
//! "what is the CRAP score of this module / type / method?" by qualified name.
//! It depends on no other CodeUML crate, so views and future analyses can use
//! it without coupling to the model.
//!
//! Aggregation: a method has the worst score among same-named functions on its
//! type; a type has the worst score of its methods; a module has the worst
//! score of everything declared in it or in its descendants. Taking the worst
//! keeps a single bad function visible from every level above it.
//!
//! The report names crates with underscores (`codeuml_tui::app::App`) while
//! package names may contain hyphens (`codeuml-tui`); lookups normalise the
//! leading crate segment.

mod config;
mod report;

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use thiserror::Error;

pub use config::{
    parse_config, CrapLevel, CrapThresholds, MetricsConfig, CONFIG_FILE, DEFAULT_REPORT,
};
pub use report::{parse_report, CrapEntry};

const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum MetricsError {
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Report(String),
    #[error("could not read `{path}`: {message}")]
    Io { path: String, message: String },
}

/// Worst-case CRAP scores indexed for fast lookup.
#[derive(Clone, Debug)]
pub struct Metrics {
    thresholds: CrapThresholds,
    methods: HashMap<(String, String), f64>,
    namespaces: BTreeMap<String, f64>,
}

impl Metrics {
    pub fn new(entries: &[CrapEntry], thresholds: CrapThresholds) -> Self {
        let mut methods: HashMap<(String, String), f64> = HashMap::new();
        let mut namespaces: BTreeMap<String, f64> = BTreeMap::new();
        for entry in entries {
            let key = (entry.namespace.clone(), entry.name.clone());
            keep_worst(methods.entry(key), entry.crap);
            let worst = namespaces
                .entry(entry.namespace.clone())
                .or_insert(entry.crap);
            *worst = worst.max(entry.crap);
        }
        Self {
            thresholds,
            methods,
            namespaces,
        }
    }

    pub fn thresholds(&self) -> CrapThresholds {
        self.thresholds
    }

    pub fn level(&self, score: f64) -> CrapLevel {
        self.thresholds.level(score)
    }

    /// Score of `method` declared on the type with this qualified name.
    pub fn method_score(&self, type_name: &str, method: &str) -> Option<f64> {
        let key = (normalize(type_name), method.to_owned());
        self.methods.get(&key).copied()
    }

    /// Worst score among a type's methods.
    pub fn type_score(&self, type_name: &str) -> Option<f64> {
        self.namespaces.get(&normalize(type_name)).copied()
    }

    /// Worst score among everything in a module and its descendants.
    pub fn module_score(&self, module_name: &str) -> Option<f64> {
        let key = normalize(module_name);
        let prefix = format!("{key}::");
        let nested = self
            .namespaces
            .range(prefix.clone()..)
            .take_while(|(namespace, _)| namespace.starts_with(&prefix))
            .map(|(_, score)| *score);
        self.namespaces
            .get(&key)
            .copied()
            .into_iter()
            .chain(nested)
            .reduce(f64::max)
    }
}

fn keep_worst<K>(slot: Entry<'_, K, f64>, score: f64) {
    slot.and_modify(|worst| *worst = worst.max(score))
        .or_insert(score);
}

fn normalize(qualified_name: &str) -> String {
    match qualified_name.split_once("::") {
        Some((crate_name, rest)) => format!("{}::{rest}", crate_name.replace('-', "_")),
        None => qualified_name.replace('-', "_"),
    }
}

/// The overlay for a project, plus anything the user should be told about.
#[derive(Debug, Default)]
pub struct Loaded {
    pub metrics: Option<Metrics>,
    pub warnings: Vec<String>,
}

/// Load `codeuml.toml` and the CRAP report from a project root.
///
/// A missing config or a missing default report is normal and silent. Broken
/// config or report files fall back to defaults/no scores with a warning, so a
/// bad metrics file never prevents browsing the code.
pub fn load(project_root: &Path) -> Loaded {
    let mut warnings = Vec::new();
    let config = read_config(project_root, &mut warnings);
    let report_path = project_root.join(&config.report);
    let metrics = read_report(&report_path, &config, &mut warnings)
        .map(|entries| Metrics::new(&entries, config.thresholds));
    Loaded { metrics, warnings }
}

fn read_config(root: &Path, warnings: &mut Vec<String>) -> MetricsConfig {
    let path = root.join(CONFIG_FILE);
    if !path.is_file() {
        return MetricsConfig::default();
    }
    let parsed = read_text(&path).and_then(|text| parse_config(&text));
    parsed.unwrap_or_else(|error| {
        warnings.push(format!("{CONFIG_FILE}: {error}; using defaults"));
        MetricsConfig::default()
    })
}

fn read_report(
    path: &Path,
    config: &MetricsConfig,
    warnings: &mut Vec<String>,
) -> Option<Vec<CrapEntry>> {
    if !path.is_file() {
        if config.report_configured {
            warnings.push(format!("CRAP report not found: {}", path.display()));
        }
        return None;
    }
    match read_text(path).and_then(|text| parse_report(&text)) {
        Ok(entries) => Some(entries),
        Err(error) => {
            warnings.push(format!("CRAP report ignored: {error}"));
            None
        }
    }
}

fn read_text(path: &Path) -> Result<String, MetricsError> {
    let io_error = |error: std::io::Error| MetricsError::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    };
    let size = fs::metadata(path).map_err(io_error)?.len();
    if size > MAX_FILE_BYTES {
        return Err(MetricsError::Report(format!(
            "`{}` is larger than {} MiB",
            path.display(),
            MAX_FILE_BYTES / (1024 * 1024)
        )));
    }
    fs::read_to_string(path).map_err(io_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(namespace: &str, name: &str, crap: f64) -> CrapEntry {
        CrapEntry {
            name: name.to_owned(),
            namespace: namespace.to_owned(),
            crap,
        }
    }

    fn sample() -> Metrics {
        Metrics::new(
            &[
                entry("my_crate::app::App", "handle", 11.0),
                entry("my_crate::app::App", "handle", 3.0),
                entry("my_crate::app::App", "new", 2.0),
                entry("my_crate::app", "helper", 5.0),
                entry("my_crate::app::nested", "deep", 40.0),
                entry("my_crate::app_extra", "other", 99.0),
                entry("my_crate", "root_fn", 1.0),
            ],
            CrapThresholds::default(),
        )
    }

    #[test]
    fn lookups_aggregate_to_the_worst_score_and_normalise_crate_names() {
        let metrics = sample();
        assert_eq!(
            metrics.method_score("my-crate::app::App", "handle"),
            Some(11.0)
        );
        assert_eq!(metrics.method_score("my-crate::app::App", "new"), Some(2.0));
        assert_eq!(metrics.method_score("my-crate::app::App", "nope"), None);
        assert_eq!(metrics.type_score("my-crate::app::App"), Some(11.0));
        assert_eq!(metrics.type_score("my-crate::app::Missing"), None);

        assert_eq!(metrics.module_score("my-crate::app::nested"), Some(40.0));
        assert_eq!(metrics.module_score("my-crate::app"), Some(40.0));
        assert_eq!(metrics.module_score("my_crate"), Some(99.0));
        assert_eq!(metrics.module_score("my-crate::absent"), None);
        assert_eq!(metrics.module_score("other"), None);
    }

    #[test]
    fn module_scores_do_not_leak_between_name_prefixes() {
        let metrics = Metrics::new(
            &[entry("c::app", "a", 4.0), entry("c::app_extra", "b", 90.0)],
            CrapThresholds::default(),
        );
        assert_eq!(metrics.module_score("c::app"), Some(4.0));
        assert_eq!(metrics.thresholds(), CrapThresholds::default());
        assert_eq!(metrics.level(90.0), CrapLevel::Bad);
    }

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "codeuml-metrics-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_nanos())
                    .unwrap_or_default()
            ));
            fs::create_dir_all(&path).expect("temp dir");
            Self(path)
        }

        fn write(&self, relative: &str, text: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            fs::write(path, text).expect("write");
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const REPORT: &str = r#"{:entries [{:name "f", :namespace "p::m", :crap 7.5}]}"#;

    #[test]
    fn load_reads_default_report_and_config_thresholds() {
        let dir = TempDir::new("load");
        dir.write(".metrics/crap.edn", REPORT);
        let loaded = load(&dir.0);
        assert!(loaded.warnings.is_empty());
        let metrics = loaded.metrics.expect("metrics");
        assert_eq!(metrics.method_score("p::m", "f"), Some(7.5));
        assert_eq!(metrics.level(7.5), CrapLevel::Medium);

        dir.write(CONFIG_FILE, "[crap]\ngood_max = 8\nreport = \"r/c.edn\"");
        dir.write("r/c.edn", REPORT);
        let loaded = load(&dir.0);
        assert_eq!(loaded.metrics.expect("metrics").level(7.5), CrapLevel::Good);
    }

    #[test]
    fn missing_default_report_is_silent_but_a_configured_one_warns() {
        let dir = TempDir::new("missing");
        let loaded = load(&dir.0);
        assert!(loaded.metrics.is_none() && loaded.warnings.is_empty());

        dir.write(CONFIG_FILE, "[crap]\nreport = \"nope.edn\"");
        let loaded = load(&dir.0);
        assert!(loaded.metrics.is_none());
        assert!(
            loaded.warnings[0].contains("not found"),
            "{:?}",
            loaded.warnings
        );
    }

    #[test]
    fn broken_files_fall_back_with_warnings() {
        let dir = TempDir::new("broken");
        dir.write(CONFIG_FILE, "[crap\n");
        dir.write(".metrics/crap.edn", r#"{:name "unterminated"#);
        let loaded = load(&dir.0);
        assert!(loaded.metrics.is_none());
        assert_eq!(loaded.warnings.len(), 2, "{:?}", loaded.warnings);
        assert!(loaded.warnings[0].contains("using defaults"));
        assert!(loaded.warnings[1].contains("ignored"));
    }

    #[test]
    fn oversized_files_are_refused() {
        let dir = TempDir::new("big");
        let path = dir.0.join("big.edn");
        let file = fs::File::create(&path).expect("file");
        file.set_len(MAX_FILE_BYTES + 1).expect("sparse");
        let error = read_text(&path).expect_err("too large");
        assert!(error.to_string().contains("larger than"), "{error}");
        let error = read_text(&dir.0.join("absent")).expect_err("absent");
        assert!(error.to_string().contains("could not read"), "{error}");
    }
}

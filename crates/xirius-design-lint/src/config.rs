//! `design-lint.toml`（ワークスペースの根）。
//!
//! ```toml
//! # 規則の持ち主の crate。この中は検査しない。
//! design_crates = ["xirius-design", "xirius-design-lint"]
//!
//! # 一時的に外す crate。理由が要る。直したら消す。
//! [[exempt]]
//! crate = "xirius-ui"
//! reason = "…"
//! ```

use std::fs;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_design_crates")]
    pub design_crates: Vec<String>,
    /// `tests/` も検査するか。
    #[serde(default)]
    pub include_tests: bool,
    #[serde(default)]
    pub exempt: Vec<Exempt>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exempt {
    #[serde(rename = "crate")]
    pub krate: String,
    pub reason: String,
}

fn default_design_crates() -> Vec<String> {
    vec!["xirius-design".into(), "xirius-design-lint".into()]
}

impl Default for Config {
    fn default() -> Self {
        Config {
            design_crates: default_design_crates(),
            include_tests: false,
            exempt: Vec::new(),
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Config, String> {
        let config: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        for e in &config.exempt {
            if e.reason.trim().is_empty() {
                return Err(format!("exempt の `{}` に理由が無い", e.krate));
            }
        }
        Ok(config)
    }

    /// ファイルが無ければ既定の設定。
    pub fn load(path: &Path) -> Result<Config, String> {
        match fs::read_to_string(path) {
            Ok(text) => Config::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
}

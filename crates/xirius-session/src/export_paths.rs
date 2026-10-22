//! Export destinations use the same absolute form for reservation and file access.
use std::path::{Path, PathBuf};
use xirius_core::{CmdError, CmdResult};

pub(crate) fn absolute(path: &Path) -> CmdResult<PathBuf> {
    std::path::absolute(path).map_err(|e| CmdError::conflict(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, json};
    use xirius_core::{command::Args, model::Project};

    #[test]
    fn a_relative_destination_obeys_an_absolute_reservation_before_the_file_exists() {
        let name = format!("../../target/export-reserved-{}.wav", std::process::id());
        let busy = absolute(Path::new(&name)).unwrap();
        assert!(!busy.exists());
        let plan = crate::commands::plan(
            &Project::new("reserve"),
            None,
            false,
            &Args(json!({"out":name}).as_object().unwrap().clone()),
            &Map::new(),
            std::slice::from_ref(&busy),
        )
        .unwrap();
        assert!(plan.target.is_absolute());
        assert_ne!(plan.target, busy);
        assert_eq!(plan.stored["out"], plan.target.display().to_string());
    }
}

//! The target guard's alerts for `loom status`, the TUI and the web
//! dashboard: one warning per recorded hold, or one naming the guard record
//! when it cannot be read, since an unreadable record holds every target.

use std::path::Path;

use crate::git::target_guard::{hold_alert, recorded_holds, RECORD_FILE};

use super::{Alert, Severity};

/// One warning per hold the guard record in `work_dir` names; one naming
/// the record when it cannot be read.
pub(super) fn guard_alerts(work_dir: &Path) -> Vec<Alert> {
    let holds = match recorded_holds(work_dir) {
        Ok(holds) => holds,
        Err(error) => return vec![unreadable_record_alert(work_dir, &error)],
    };
    holds
        .iter()
        .map(|(target, hold)| Alert {
            severity: Severity::Warning,
            text: hold_alert(target, hold),
        })
        .collect()
}

fn unreadable_record_alert(work_dir: &Path, error: &anyhow::Error) -> Alert {
    Alert {
        severity: Severity::Warning,
        text: format!(
            "target guard record {} cannot be read ({}); merges into the target and \
             stages cut from it wait until it reads again (loom target status)",
            work_dir.join(RECORD_FILE).display(),
            error.root_cause()
        ),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::super::{alerts, Severity};
    use super::RECORD_FILE;

    #[test]
    fn an_unreadable_guard_record_is_one_warning_naming_it() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join(RECORD_FILE), "{ not json").unwrap();

        for daemon_running in [false, true] {
            let shown = alerts(temp.path(), daemon_running);

            assert_eq!(shown.len(), 1, "{shown:?}");
            assert_eq!(shown[0].severity, Severity::Warning);
            assert!(shown[0].text.contains(RECORD_FILE), "{shown:?}");
            assert!(
                shown[0].text.contains("merges into the target"),
                "{shown:?}"
            );
        }
    }

    #[test]
    fn a_record_without_holds_raises_no_guard_alert() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join(RECORD_FILE), r#"{"targets":{}}"#).unwrap();

        assert!(alerts(temp.path(), false).is_empty());
    }
}

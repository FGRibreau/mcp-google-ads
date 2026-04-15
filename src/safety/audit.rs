use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use chrono::Utc;
use serde_json::json;

use crate::error::Result;

/// Expand ~ to home directory
fn expand_path(path: &str) -> PathBuf {
    if path.starts_with("~/") || path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]);
        }
    }
    PathBuf::from(path)
}

/// Parameters for logging a mutation to the audit log.
pub struct MutationLogEntry<'a> {
    pub log_file: &'a str,
    pub operation: &'a str,
    pub customer_id: &'a str,
    pub entity_type: &'a str,
    pub entity_id: &'a str,
    pub changes: &'a serde_json::Value,
    pub dry_run: bool,
    pub result: &'a str,
    pub error: &'a str,
}

/// Append a mutation record to the audit log file
pub fn log_mutation(entry: &MutationLogEntry) -> Result<()> {
    let path = expand_path(entry.log_file);
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }

    let record = json!({
        "timestamp": Utc::now().to_rfc3339(),
        "operation": entry.operation,
        "customer_id": entry.customer_id,
        "entity_type": entry.entity_type,
        "entity_id": entry.entity_id,
        "changes": entry.changes,
        "dry_run": entry.dry_run,
        "result": entry.result,
        "error": entry.error,
    });

    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{}", serde_json::to_string(&record)?)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_log_path() -> PathBuf {
        let dir = std::env::temp_dir().join("mcp-google-ads-test");
        fs::create_dir_all(&dir).ok();
        dir.join(format!("audit-{}.log", uuid::Uuid::new_v4()))
    }

    #[test]
    fn test_log_mutation_creates_file() {
        let path = temp_log_path();
        let path_str = path.to_string_lossy().to_string();
        let changes = serde_json::json!({"budget": 10.0});
        let entry = MutationLogEntry {
            log_file: &path_str,
            operation: "test_op",
            customer_id: "1234567890",
            entity_type: "campaign",
            entity_id: "555",
            changes: &changes,
            dry_run: false,
            result: "SUCCESS",
            error: "",
        };

        let result = log_mutation(&entry);
        assert!(result.is_ok());
        assert!(path.exists());

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_log_mutation_appends() {
        let path = temp_log_path();
        let path_str = path.to_string_lossy().to_string();
        let changes = serde_json::json!({"budget": 10.0});

        let entry1 = MutationLogEntry {
            log_file: &path_str,
            operation: "test_op",
            customer_id: "1234567890",
            entity_type: "campaign",
            entity_id: "555",
            changes: &changes,
            dry_run: false,
            result: "SUCCESS",
            error: "",
        };
        log_mutation(&entry1).ok();

        let entry2 = MutationLogEntry {
            log_file: &path_str,
            operation: "test_op_2",
            customer_id: "1234567890",
            entity_type: "campaign",
            entity_id: "666",
            changes: &changes,
            dry_run: false,
            result: "SUCCESS",
            error: "",
        };
        log_mutation(&entry2).ok();

        let contents = fs::read_to_string(&path).unwrap_or_default();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);

        fs::remove_file(&path).ok();
    }

    #[test]
    fn test_log_mutation_json_format() {
        let path = temp_log_path();
        let path_str = path.to_string_lossy().to_string();
        let changes = serde_json::json!({"budget": 10.0});

        let entry = MutationLogEntry {
            log_file: &path_str,
            operation: "test_op",
            customer_id: "1234567890",
            entity_type: "campaign",
            entity_id: "555",
            changes: &changes,
            dry_run: false,
            result: "SUCCESS",
            error: "",
        };
        log_mutation(&entry).ok();

        let contents = fs::read_to_string(&path).unwrap_or_default();
        let parsed: serde_json::Value =
            serde_json::from_str(contents.lines().next().unwrap_or_default()).unwrap_or_default();

        assert!(parsed.get("timestamp").is_some());
        assert_eq!(parsed["operation"], "test_op");
        assert_eq!(parsed["customer_id"], "1234567890");
        assert_eq!(parsed["entity_type"], "campaign");
        assert_eq!(parsed["entity_id"], "555");
        assert_eq!(parsed["dry_run"], false);
        assert_eq!(parsed["result"], "SUCCESS");

        fs::remove_file(&path).ok();
    }
}

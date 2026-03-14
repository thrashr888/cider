use super::util::{run_command_with_timeout, ActionResult};
use serde::Serialize;
use std::time::Duration;
use tokio::process::Command;

#[derive(Debug, Serialize)]
pub struct Shortcut {
    pub name: String,
}

pub async fn list() -> anyhow::Result<Vec<Shortcut>> {
    let output =
        run_command_with_timeout("shortcuts", &["list"], std::time::Duration::from_secs(15))
            .await?;

    Ok(parse_output(&output))
}

pub async fn run(name: &str, input: Option<&str>) -> anyhow::Result<ActionResult> {
    let timeout = Duration::from_secs(120);

    let output = if let Some(input_text) = input {
        // Pipe input via stdin
        let mut child = Command::new("shortcuts")
            .args(["run", name])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(input_text.as_bytes()).await?;
            // Drop stdin to close it so the shortcut can proceed
        }

        tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| anyhow::anyhow!("shortcuts timed out after {timeout:?}"))??
    } else {
        let child = Command::new("shortcuts")
            .args(["run", name])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| anyhow::anyhow!("shortcuts timed out after {timeout:?}"))??
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("shortcuts run failed: {stderr}");
    }

    let stdout = String::from_utf8(output.stdout)?.trim().to_string();
    if stdout.is_empty() {
        Ok(ActionResult::success_with_message(
            "run",
            &format!("Ran shortcut '{name}'"),
        ))
    } else {
        Ok(ActionResult::success_with_message("run", &stdout))
    }
}

fn parse_output(output: &str) -> Vec<Shortcut> {
    output
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| Shortcut {
            name: l.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_output() {
        let output = "Morning Routine\nOpen Apps\nSend ETA\n";
        let records = parse_output(output);
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].name, "Morning Routine");
    }

    #[test]
    fn test_parse_output_empty() {
        assert!(parse_output("").is_empty());
    }
}

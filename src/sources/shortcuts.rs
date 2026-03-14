use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Shortcut {
    pub name: String,
}

pub async fn fetch() -> anyhow::Result<Vec<Shortcut>> {
    let output =
        run_command_with_timeout("shortcuts", &["list"], std::time::Duration::from_secs(15))
            .await?;

    Ok(parse_output(&output))
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

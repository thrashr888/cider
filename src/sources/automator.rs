use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Workflow {
    pub name: String,
    pub path: String,
}

pub async fn fetch() -> anyhow::Result<Vec<Workflow>> {
    // Find all Automator workflows and Quick Actions
    let output = run_command_with_timeout(
        "mdfind",
        &["kMDItemContentType == 'com.apple.automator-workflow'"],
        std::time::Duration::from_secs(10),
    )
    .await?;

    let mut workflows: Vec<Workflow> = output
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|path| {
            let name = path
                .rsplit('/')
                .next()
                .unwrap_or(path)
                .strip_suffix(".workflow")
                .unwrap_or(path)
                .to_string();
            Workflow {
                name,
                path: path.to_string(),
            }
        })
        .collect();

    workflows.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(workflows)
}

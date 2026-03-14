use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SpotlightResult {
    pub path: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Search for files using Spotlight (mdfind).
pub async fn search(query: &str, directory: Option<&str>) -> anyhow::Result<Vec<SpotlightResult>> {
    let mut args = vec![query];
    if let Some(dir) = directory {
        args.push("-onlyin");
        args.push(dir);
    }

    let output =
        run_command_with_timeout("mdfind", &args, std::time::Duration::from_secs(15)).await?;

    let results: Vec<SpotlightResult> = output
        .lines()
        .take(200)
        .filter(|l| !l.trim().is_empty())
        .map(|path| {
            let path = path.trim();
            let name = path.rsplit('/').next().unwrap_or(path).to_string();
            let kind = infer_kind(path);
            SpotlightResult {
                path: path.to_string(),
                name,
                kind,
            }
        })
        .collect();

    Ok(results)
}

fn infer_kind(path: &str) -> Option<String> {
    let ext = path.rsplit('.').next()?.to_lowercase();
    let kind = match ext.as_str() {
        "pdf" => "PDF",
        "doc" | "docx" => "Document",
        "xls" | "xlsx" => "Spreadsheet",
        "ppt" | "pptx" => "Presentation",
        "jpg" | "jpeg" | "png" | "gif" | "heic" | "webp" => "Image",
        "mov" | "mp4" | "avi" | "mkv" => "Video",
        "mp3" | "m4a" | "wav" | "flac" | "aac" => "Audio",
        "zip" | "tar" | "gz" | "dmg" => "Archive",
        "app" => "Application",
        "rs" | "py" | "js" | "ts" | "rb" | "go" | "swift" | "c" | "cpp" | "java" => "Source Code",
        "md" | "txt" | "rtf" => "Text",
        "html" | "htm" => "Web Page",
        "json" | "yaml" | "yml" | "toml" | "xml" => "Data",
        _ => return None,
    };
    Some(kind.to_string())
}

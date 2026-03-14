use super::util::run_command_with_timeout;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SystemActivity {
    pub cpu_usage_percent: f64,
    pub memory_used_gb: f64,
    pub memory_total_gb: f64,
    pub load_average: String,
    pub processes_total: i64,
    pub top_processes: Vec<ProcessInfo>,
}

#[derive(Debug, Serialize)]
pub struct ProcessInfo {
    pub pid: i64,
    pub name: String,
    pub cpu_percent: f64,
    pub memory_mb: f64,
}

pub async fn fetch() -> anyhow::Result<Vec<SystemActivity>> {
    let timeout = std::time::Duration::from_secs(10);

    // Get top processes by CPU
    let top_output =
        run_command_with_timeout("ps", &["-eo", "pid,pcpu,rss,comm", "-r"], timeout).await?;

    let mut processes = Vec::new();
    for line in top_output.lines().skip(1).take(15) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let pid: i64 = parts[0].parse().unwrap_or(0);
        let cpu: f64 = parts[1].parse().unwrap_or(0.0);
        let rss_kb: f64 = parts[2].parse().unwrap_or(0.0);
        let name = parts[3..].join(" ");
        let name = name.rsplit('/').next().unwrap_or(&name).to_string();

        processes.push(ProcessInfo {
            pid,
            name,
            cpu_percent: cpu,
            memory_mb: rss_kb / 1024.0,
        });
    }

    // Memory info
    let mem_total: f64 = run_command_with_timeout("sysctl", &["-n", "hw.memsize"], timeout)
        .await
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|b| b as f64 / 1_073_741_824.0)
        .unwrap_or(0.0);

    let vm_stat = run_command_with_timeout("vm_stat", &[], timeout).await.ok();
    let pages_active = extract_vm_stat_value(&vm_stat, "Pages active");
    let pages_wired = extract_vm_stat_value(&vm_stat, "Pages wired");
    let pages_compressed = extract_vm_stat_value(&vm_stat, "Pages occupied by compressor");
    let page_size: f64 = 16384.0; // Apple Silicon default
    let mem_used = (pages_active + pages_wired + pages_compressed) * page_size / 1_073_741_824.0;

    // Load average
    let load = run_command_with_timeout("sysctl", &["-n", "vm.loadavg"], timeout)
        .await
        .unwrap_or_default()
        .trim()
        .trim_start_matches("{ ")
        .trim_end_matches(" }")
        .to_string();

    // Total CPU from top processes
    let cpu_total: f64 = processes.iter().map(|p| p.cpu_percent).sum();

    // Process count
    let proc_count: i64 = run_command_with_timeout("ps", &["-e"], timeout)
        .await
        .ok()
        .map(|s| s.lines().count() as i64 - 1)
        .unwrap_or(0);

    Ok(vec![SystemActivity {
        cpu_usage_percent: cpu_total,
        memory_used_gb: (mem_used * 100.0).round() / 100.0,
        memory_total_gb: mem_total,
        load_average: load,
        processes_total: proc_count,
        top_processes: processes,
    }])
}

fn extract_vm_stat_value(vm_stat: &Option<String>, key: &str) -> f64 {
    vm_stat
        .as_ref()
        .and_then(|s| {
            s.lines().find(|l| l.contains(key)).and_then(|l| {
                l.split(':')
                    .nth(1)
                    .map(|v| v.trim().trim_end_matches('.'))
                    .and_then(|v| v.parse::<f64>().ok())
            })
        })
        .unwrap_or(0.0)
}

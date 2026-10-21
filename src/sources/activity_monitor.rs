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

#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub pid: i64,
    pub name: String,
    pub cpu_percent: f64,
    pub memory_mb: f64,
}

/// Activity Monitor's numbers: CPU, memory, swap and load, and the busiest processes.
#[derive(Debug, Serialize)]
pub struct ActivitySnapshot {
    pub cpu_cores: u32,
    /// All processes' CPU over all cores, 0-100.
    pub cpu_percent: f64,
    pub load_average: [f64; 3],
    pub memory_total_gb: f64,
    /// Active, wired and compressed pages, like Activity Monitor's Memory Used.
    pub memory_used_gb: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_free_percent: Option<u32>,
    pub swap_used_gb: f64,
    pub processes_total: usize,
    pub sort: Sort,
    pub processes: Vec<ProcessRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProcessRow {
    pub pid: i64,
    pub name: String,
    pub user: String,
    pub cpu_percent: f64,
    pub memory_mb: f64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    #[default]
    Cpu,
    Memory,
}

const GB: f64 = 1_073_741_824.0;

pub async fn fetch() -> anyhow::Result<Vec<SystemActivity>> {
    let s = snapshot(Sort::Cpu, 15).await?;
    Ok(vec![SystemActivity {
        cpu_usage_percent: s.cpu_percent,
        memory_used_gb: s.memory_used_gb,
        memory_total_gb: s.memory_total_gb,
        load_average: s
            .load_average
            .iter()
            .map(|l| format!("{l:.2}"))
            .collect::<Vec<_>>()
            .join(" "),
        processes_total: s.processes_total as i64,
        top_processes: s
            .processes
            .into_iter()
            .map(|p| ProcessInfo {
                pid: p.pid,
                name: p.name,
                cpu_percent: p.cpu_percent,
                memory_mb: p.memory_mb,
            })
            .collect(),
    }])
}

/// One look at the machine, with the top `limit` processes by CPU or memory.
pub async fn snapshot(sort: Sort, limit: usize) -> anyhow::Result<ActivitySnapshot> {
    let timeout = std::time::Duration::from_secs(10);
    let sysctl = |name: &'static str| async move {
        run_command_with_timeout("sysctl", &["-n", name], timeout)
            .await
            .ok()
            .map(|s| s.trim().to_string())
    };

    let ps = run_command_with_timeout("ps", &["-A", "-o", "pid=,user=,pcpu=,rss=,comm="], timeout)
        .await?;
    let mut processes = parse_ps(&ps);
    let processes_total = processes.len();
    let cores: u32 = sysctl("hw.logicalcpu")
        .await
        .and_then(|s| s.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(1);
    let cpu_sum: f64 = processes.iter().map(|p| p.cpu_percent).sum();
    sort_processes(&mut processes, sort);
    processes.truncate(limit);

    let memory_total_gb = sysctl("hw.memsize")
        .await
        .and_then(|s| s.parse::<u64>().ok())
        .map(|b| b as f64 / GB)
        .unwrap_or(0.0);
    let vm_stat = run_command_with_timeout("vm_stat", &[], timeout)
        .await
        .unwrap_or_default();
    let swap = sysctl("vm.swapusage").await.unwrap_or_default();

    Ok(ActivitySnapshot {
        cpu_cores: cores,
        cpu_percent: round1((cpu_sum / cores as f64).min(100.0)),
        load_average: parse_load(&sysctl("vm.loadavg").await.unwrap_or_default()),
        memory_total_gb: round2(memory_total_gb),
        memory_used_gb: round2(memory_used(&vm_stat) / GB),
        memory_free_percent: sysctl("kern.memorystatus_level")
            .await
            .and_then(|s| s.parse().ok()),
        swap_used_gb: round2(swap_used_mb(&swap) / 1024.0),
        processes_total,
        sort,
        processes,
    })
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// `ps -A -o pid=,user=,pcpu=,rss=,comm=`: the command is last and may hold spaces.
fn parse_ps(output: &str) -> Vec<ProcessRow> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let user = parts.next()?.to_string();
            let cpu: f64 = parts.next()?.parse().ok()?;
            let rss_kb: f64 = parts.next()?.parse().ok()?;
            let comm = parts.collect::<Vec<_>>().join(" ");
            let name = comm.rsplit('/').next().unwrap_or(&comm).to_string();
            (!name.is_empty()).then(|| ProcessRow {
                pid,
                name,
                user,
                cpu_percent: cpu,
                memory_mb: round1(rss_kb / 1024.0),
            })
        })
        .collect()
}

fn sort_processes(processes: &mut [ProcessRow], sort: Sort) {
    let key = |p: &ProcessRow| match sort {
        Sort::Cpu => p.cpu_percent,
        Sort::Memory => p.memory_mb,
    };
    processes.sort_by(|a, b| key(b).total_cmp(&key(a)).then(a.pid.cmp(&b.pid)));
}

/// `{ 1.84 2.01 2.10 }`
fn parse_load(output: &str) -> [f64; 3] {
    let mut load = [0.0; 3];
    for (slot, value) in load.iter_mut().zip(
        output
            .split(|c: char| c.is_whitespace() || c == '{' || c == '}')
            .filter_map(|s| s.parse::<f64>().ok()),
    ) {
        *slot = value;
    }
    load
}

/// Bytes in active, wired and compressed pages, at the page size vm_stat reports.
fn memory_used(vm_stat: &str) -> f64 {
    let page_size = vm_stat
        .lines()
        .next()
        .and_then(|l| l.split("page size of ").nth(1))
        .and_then(|l| l.split_whitespace().next())
        .and_then(|n| n.parse::<f64>().ok())
        .unwrap_or(16384.0);
    let pages = |key: &str| {
        vm_stat
            .lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split(':').nth(1))
            .and_then(|v| v.trim().trim_end_matches('.').parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    (pages("Pages active") + pages("Pages wired down") + pages("Pages occupied by compressor"))
        * page_size
}

/// `total = 2048.00M  used = 1024.50M  free = 1023.50M  (encrypted)`
fn swap_used_mb(output: &str) -> f64 {
    output
        .split("used = ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|v| {
            let (n, unit) = v.split_at(v.len().saturating_sub(1));
            let n: f64 = n.parse().ok()?;
            Some(match unit {
                "G" => n * 1024.0,
                "K" => n / 1024.0,
                _ => n,
            })
        })
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ps_with_spaces_in_names() {
        let rows = parse_ps(
            "  1 root   0.3  12288 /sbin/launchd\n\
             812 paul  41.5 524288 /Applications/Google Chrome.app/Contents/MacOS/Google Chrome\n\
             garbage line\n",
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].name, "Google Chrome");
        assert_eq!(rows[1].user, "paul");
        assert_eq!(rows[1].memory_mb, 512.0);
    }

    #[test]
    fn sorts_by_cpu_or_memory() {
        let mut rows = parse_ps("1 a 5.0 100 x\n2 a 1.0 900 y\n3 a 9.0 50 z\n");
        sort_processes(&mut rows, Sort::Cpu);
        assert_eq!(rows.iter().map(|r| r.pid).collect::<Vec<_>>(), [3, 1, 2]);
        sort_processes(&mut rows, Sort::Memory);
        assert_eq!(rows[0].pid, 2);
    }

    #[test]
    fn parses_load_memory_and_swap() {
        assert_eq!(parse_load("{ 1.84 2.01 2.10 }"), [1.84, 2.01, 2.10]);
        let vm = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
                  Pages free:                               1000.\n\
                  Pages active:                            65536.\n\
                  Pages wired down:                        65536.\n\
                  Pages occupied by compressor:                0.\n";
        assert_eq!(memory_used(vm), 2.0 * GB);
        assert_eq!(
            swap_used_mb("total = 2048.00M  used = 1.50G  free = 548.00M  (encrypted)"),
            1536.0
        );
        assert_eq!(swap_used_mb(""), 0.0);
    }
}

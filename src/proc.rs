use std::fs;
use std::path::Path;

#[cfg(target_os = "linux")]
fn read_cmdline(pid: u32) -> Option<Vec<String>> {
    let path = format!("/proc/{}/cmdline", pid);
    let data = fs::read(&path).ok()?;
    let args: Vec<String> = data
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    if args.is_empty() {
        None
    } else {
        Some(args)
    }
}

#[cfg(target_os = "linux")]
fn read_cwd(pid: u32) -> Option<String> {
    let path = format!("/proc/{}/cwd", pid);
    fs::read_link(&path)
        .ok()
        .and_then(|p| p.into_os_string().into_string().ok())
}

#[cfg(target_os = "linux")]
fn read_comm(pid: u32) -> Option<String> {
    let path = format!("/proc/{}/comm", pid);
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

#[cfg(target_os = "linux")]
fn get_children(pid: u32) -> Vec<u32> {
    let path = format!("/proc/{}/task/{}/children", pid, pid);
    fs::read_to_string(&path)
        .ok()
        .map(|s| {
            s.split_whitespace()
                .filter_map(|p| p.parse::<u32>().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn is_shell(comm: &str, shell_names: &[String]) -> bool {
    shell_names.iter().any(|s| s == comm)
}

fn is_helper(comm: &str, helper_names: &[String]) -> bool {
    helper_names.iter().any(|s| s == comm)
}

#[cfg(target_os = "linux")]
fn read_stat_field(pid: u32, field: usize) -> Option<i64> {
    let path = format!("/proc/{}/stat", pid);
    let data = fs::read_to_string(&path).ok()?;

    let comm_end = data.find(')')?;
    let after_comm = &data[comm_end + 2..];
    let fields: Vec<&str> = after_comm.split_whitespace().collect();

    let idx = field.saturating_sub(3);
    fields.get(idx).and_then(|f| f.parse::<i64>().ok())
}

#[cfg(target_os = "linux")]
pub fn resolve_child_process(
    pid: u32,
    shell_names: &[String],
    helper_names: &[String],
    max_depth: u32,
) -> Option<(String, String)> {
    let mut current = pid;

    for _ in 0..max_depth {
        if !Path::new(&format!("/proc/{}", current)).exists() {
            break;
        }

        let children = get_children(current);

        if children.is_empty() {
            let tpgid = read_stat_field(current, 8).unwrap_or(0) as u32;
            if tpgid > 0 {
                if let Some(fg_comm) = read_comm(tpgid) {
                    if !is_shell(&fg_comm, shell_names)
                        && fg_comm != "__atexit__"
                        && !is_helper(&fg_comm, helper_names)
                    {
                        let cmd = read_cmdline(tpgid)
                            .map(|args| args.join(" "))
                            .unwrap_or_default();
                        let cwd = read_cwd(tpgid).unwrap_or_default();
                        return Some((cmd, cwd));
                    }
                }
            }
            break;
        }

        current = children[0];
        let comm = read_comm(current)?;

        if is_shell(&comm, shell_names) || is_helper(&comm, helper_names) {
            continue;
        }

        let cmd = read_cmdline(current)
            .map(|args| args.join(" "))
            .unwrap_or_default();
        let cwd = read_cwd(current).unwrap_or_default();
        return Some((cmd, cwd));
    }

    None
}

#[cfg(not(target_os = "linux"))]
pub fn resolve_child_process(
    _pid: u32,
    _shell_names: &[String],
    _helper_names: &[String],
    _max_depth: u32,
) -> Option<(String, String)> {
    None
}

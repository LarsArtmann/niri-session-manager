use std::fs;
use std::path::Path;
use tracing::warn;

#[cfg(target_os = "linux")]
fn read_cmdline_at(base: &Path, pid: u32) -> Option<Vec<String>> {
    let path = base.join(pid.to_string()).join("cmdline");
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
fn read_cwd_at(base: &Path, pid: u32) -> Option<String> {
    let path = base.join(pid.to_string()).join("cwd");
    fs::read_link(&path)
        .ok()
        .and_then(|p| p.into_os_string().into_string().ok())
}

#[cfg(target_os = "linux")]
fn read_comm_at(base: &Path, pid: u32) -> Option<String> {
    let path = base.join(pid.to_string()).join("comm");
    fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
}

#[cfg(target_os = "linux")]
fn get_children_at(base: &Path, pid: u32) -> Vec<u32> {
    let path = base
        .join(pid.to_string())
        .join("task")
        .join(pid.to_string())
        .join("children");
    if let Ok(data) = fs::read_to_string(&path) {
        let children: Vec<u32> = data
            .split_whitespace()
            .filter_map(|p| p.parse::<u32>().ok())
            .collect();
        if !children.is_empty() {
            return children;
        }
    }
    // The children file can be empty for some fork shapes even though the
    // child exists and reports this pid as its ppid (observed live
    // 2026-09-16: nix's .ghostty-wrapper reported zero children while the
    // wrapped terminal's child was right there — terminal state silently
    // never captured). Scanning /proc stat ppids is the ground truth `ps`
    // itself uses.
    scan_children_by_ppid(base, pid)
}

#[cfg(target_os = "linux")]
fn scan_children_by_ppid(base: &Path, pid: u32) -> Vec<u32> {
    let Ok(entries) = fs::read_dir(base) else {
        return Vec::new();
    };
    let mut children = Vec::new();
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Ok(candidate) = name.parse::<u32>() else {
            continue;
        };
        if candidate == pid {
            continue;
        }
        if read_stat_field_at(base, candidate, 4) == Some(i64::from(pid)) {
            children.push(candidate);
        }
    }
    children.sort_unstable();
    children
}

fn is_shell(comm: &str, shell_names: &[String]) -> bool {
    shell_names.iter().any(|s| s == comm)
}

fn is_helper(comm: &str, helper_names: &[String]) -> bool {
    helper_names.iter().any(|s| s == comm)
}

#[cfg(target_os = "linux")]
fn read_stat_field_at(base: &Path, pid: u32, field: usize) -> Option<i64> {
    let path = base.join(pid.to_string()).join("stat");
    let data = fs::read_to_string(&path).ok()?;

    let comm_end = data.find(')')?;
    let after_comm = data.get(comm_end.saturating_add(2)..)?;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();

    let idx = field.saturating_sub(3);
    fields.get(idx).and_then(|f| f.parse::<i64>().ok())
}

#[cfg(target_os = "linux")]
fn resolve_child_process_at(
    base: &Path,
    pid: u32,
    shell_names: &[String],
    helper_names: &[String],
    max_depth: u32,
) -> Option<(Vec<String>, String)> {
    let mut current = pid;

    for depth in 0..max_depth {
        let proc_path = base.join(current.to_string());
        if !proc_path.exists() {
            if depth == 0 {
                warn!("[proc] PID {} no longer exists in {:?}", pid, base);
            }
            break;
        }

        let children = get_children_at(base, current);
        let tpgid = u32::try_from(read_stat_field_at(base, current, 8).unwrap_or(0)).unwrap_or(0);

        if children.is_empty() {
            if tpgid > 0 {
                if let Some(fg_comm) = read_comm_at(base, tpgid) {
                    if !is_shell(&fg_comm, shell_names)
                        && fg_comm != "__atexit__"
                        && !is_helper(&fg_comm, helper_names)
                    {
                        let cmd = read_cmdline_at(base, tpgid).unwrap_or_default();
                        let cwd = read_cwd_at(base, tpgid).unwrap_or_default();
                        return Some((cmd, cwd));
                    }
                }
            }
            break;
        }

        // Prefer the foreground child (matching tpgid); next prefer children
        // that are neither shells nor helpers (kitty always spawns leaf
        // helper kittens like `__atexit__` and `__watch_conf__` BESIDE the
        // real child, and they sort first by pid); fall back to the first
        // child. Descending into a leaf helper is a dead end.
        let next_pid = children
            .iter()
            .copied()
            .find(|&c| tpgid > 0 && c == tpgid)
            .or_else(|| {
                children.iter().copied().find(|&c| {
                    read_comm_at(base, c).is_some_and(|comm| {
                        !is_shell(&comm, shell_names) && !is_helper(&comm, helper_names)
                    })
                })
            })
            .or_else(|| children.first().copied());

        let next_pid = next_pid?;

        let Some(comm) = read_comm_at(base, next_pid) else {
            warn!(
                "[proc] could not read comm for PID {} (child of {})",
                next_pid, current
            );
            return None;
        };

        if is_shell(&comm, shell_names) || is_helper(&comm, helper_names) {
            current = next_pid;
            continue;
        }

        let cmd = read_cmdline_at(base, next_pid).unwrap_or_default();
        let cwd = read_cwd_at(base, next_pid).unwrap_or_default();
        return Some((cmd, cwd));
    }

    None
}

#[cfg(target_os = "linux")]
pub fn resolve_child_process(
    pid: u32,
    shell_names: &[String],
    helper_names: &[String],
    max_depth: u32,
) -> Option<(Vec<String>, String)> {
    resolve_child_process_at(
        Path::new("/proc"),
        pid,
        shell_names,
        helper_names,
        max_depth,
    )
}

#[cfg(not(target_os = "linux"))]
pub fn resolve_child_process(
    _pid: u32,
    _shell_names: &[String],
    _helper_names: &[String],
    _max_depth: u32,
) -> Option<(Vec<String>, String)> {
    None
}

#[cfg(test)]
// Deliberate lint exemption (see AGENTS.md "Testing"): same policy as
// src/tests.rs for the inline proc-tree fixtures.
#[allow(
    clippy::pedantic,
    clippy::nursery,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::as_conversions
)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    // The repeated kitty/fish/child setups below are deliberate: each test's
    // fake proc-tree shape is its input data, kept inline so every scenario
    // reads top-to-bottom (jscpd flags them; do not extract).
    fn create_fake_proc_dir(dir: &Path, pid: u32) -> PathBuf {
        let pid_dir = dir.join(pid.to_string());
        fs::create_dir_all(&pid_dir).unwrap();
        let task_dir = pid_dir.join("task").join(pid.to_string());
        fs::create_dir_all(&task_dir).unwrap();
        pid_dir
    }

    fn write_cmdline(dir: &Path, args: &[&str]) {
        let data: Vec<u8> = args
            .iter()
            .flat_map(|a| {
                let mut bytes = a.as_bytes().to_vec();
                bytes.push(0);
                bytes
            })
            .collect();
        fs::write(dir.join("cmdline"), data).unwrap();
    }

    fn write_comm(dir: &Path, name: &str) {
        fs::write(dir.join("comm"), format!("{name}\n")).unwrap();
    }

    fn write_children(dir: &Path, pids: &[u32]) {
        let children_path = dir
            .join("task")
            .join(dir.file_name().unwrap())
            .join("children");
        let content: String = pids.iter().map(|p| format!("{p} ")).collect();
        fs::write(children_path, content.trim()).unwrap();
    }

    /// Real shape observed live 2026-09-16 (nix .ghostty-wrapper): the
    /// children file is EMPTY, the wrapper's tpgid is -1, yet the child
    /// exists and points back via ppid.
    fn write_stat(dir: &Path, pid: u32, comm: &str, ppid: u32, tpgid: i64) {
        fs::write(
            dir.join("stat"),
            format!("{pid} ({comm}) S {ppid} {pid} {pid} 0 {tpgid} 0 0\n"),
        )
        .unwrap();
    }

    /// Real kitty shape observed live 2026-09-16: the terminal process has
    /// leaf helper kittens (`__atexit__`, `__watch_conf__`) beside the real
    /// child, sorted first by pid, and tpgid is -1 — the walk must not
    /// descend into a helper dead-end.
    #[test]
    fn resolve_prefers_non_helper_children_over_leaf_helpers() {
        let tmp = tempfile::tempdir().unwrap();

        let kitty_dir = create_fake_proc_dir(tmp.path(), 3000);
        write_cmdline(&kitty_dir, &["kitty", "sleep", "60"]);
        write_comm(&kitty_dir, ".kitty-wrapped");
        write_stat(&kitty_dir, 3000, ".kitty-wrapped", 1, -1);
        write_children(&kitty_dir, &[3001, 3002, 3003]);

        let atexit_dir = create_fake_proc_dir(tmp.path(), 3001);
        write_cmdline(&atexit_dir, &["kitten", "__atexit__"]);
        write_comm(&atexit_dir, "kitten");
        write_stat(&atexit_dir, 3001, "kitten", 3000, -1);
        write_children(&atexit_dir, &[]);

        let sleep_dir = create_fake_proc_dir(tmp.path(), 3002);
        write_cmdline(&sleep_dir, &["sleep", "60"]);
        write_comm(&sleep_dir, "sleep");
        write_stat(&sleep_dir, 3002, "sleep", 3000, 3002);
        symlink("/home/user", sleep_dir.join("cwd")).unwrap();
        write_children(&sleep_dir, &[]);

        let watch_dir = create_fake_proc_dir(tmp.path(), 3003);
        write_cmdline(&watch_dir, &["kitten", "__watch_conf__"]);
        write_comm(&watch_dir, "kitten");
        write_stat(&watch_dir, 3003, "kitten", 3000, -1);
        write_children(&watch_dir, &[]);

        let shell_names = vec!["fish".to_string()];
        let helper_names = vec!["kitten".to_string()];
        let result = resolve_child_process_at(tmp.path(), 3000, &shell_names, &helper_names, 20);
        assert_eq!(
            result,
            Some((
                vec!["sleep".to_string(), "60".to_string()],
                "/home/user".to_string()
            )),
            "the walk must skip leaf helper kittens and resolve the real child"
        );
    }

    #[test]
    fn resolve_finds_child_when_children_file_lies() {
        let tmp = tempfile::tempdir().unwrap();

        let wrapper_dir = create_fake_proc_dir(tmp.path(), 2000);
        write_cmdline(&wrapper_dir, &["ghostty", "-e", "btop"]);
        write_comm(&wrapper_dir, ".ghostty-wrapper");
        write_stat(&wrapper_dir, 2000, ".ghostty-wrapper", 1, -1);
        write_children(&wrapper_dir, &[]); // empty, as on the live system

        let btop_dir = create_fake_proc_dir(tmp.path(), 2001);
        write_cmdline(&btop_dir, &["btop"]);
        write_comm(&btop_dir, "btop");
        write_stat(&btop_dir, 2001, "btop", 2000, 2001);
        symlink("/home/user", btop_dir.join("cwd")).unwrap();
        write_children(&btop_dir, &[]);

        let shell_names = vec!["fish".to_string(), "bash".to_string()];
        let helper_names: Vec<String> = vec![];
        let result = resolve_child_process_at(tmp.path(), 2000, &shell_names, &helper_names, 20);
        assert_eq!(
            result,
            Some((vec!["btop".to_string()], "/home/user".to_string())),
            "an empty children file must fall back to scanning stat ppids"
        );
    }

    #[test]
    fn resolve_finds_direct_child() {
        let tmp = tempfile::tempdir().unwrap();

        let kitty_dir = create_fake_proc_dir(tmp.path(), 1000);
        write_cmdline(&kitty_dir, &["kitty"]);
        write_comm(&kitty_dir, "kitty");
        write_children(&kitty_dir, &[1001]);

        let fish_dir = create_fake_proc_dir(tmp.path(), 1001);
        write_cmdline(&fish_dir, &["fish"]);
        write_comm(&fish_dir, "fish");
        write_children(&fish_dir, &[1002]);

        let btop_dir = create_fake_proc_dir(tmp.path(), 1002);
        write_cmdline(&btop_dir, &["btop"]);
        write_comm(&btop_dir, "btop");
        symlink("/home/user", btop_dir.join("cwd")).unwrap();
        write_children(&btop_dir, &[]);

        let shell_names = vec!["fish".to_string(), "bash".to_string()];
        let helper_names: Vec<String> = vec![];
        let result = resolve_child_process_at(tmp.path(), 1000, &shell_names, &helper_names, 20);
        assert_eq!(
            result,
            Some((vec!["btop".to_string()], "/home/user".to_string()))
        );
    }

    #[test]
    fn resolve_skips_shell_and_finds_grandchild() {
        let tmp = tempfile::tempdir().unwrap();

        let kitty_dir = create_fake_proc_dir(tmp.path(), 1000);
        write_cmdline(&kitty_dir, &["kitty"]);
        write_comm(&kitty_dir, "kitty");
        write_children(&kitty_dir, &[1001]);

        let bash_dir = create_fake_proc_dir(tmp.path(), 1001);
        write_cmdline(&bash_dir, &["bash"]);
        write_comm(&bash_dir, "bash");
        write_children(&bash_dir, &[1002]);

        let nvim_dir = create_fake_proc_dir(tmp.path(), 1002);
        write_cmdline(&nvim_dir, &["nvim", "/path/to/file"]);
        write_comm(&nvim_dir, "nvim");
        symlink("/home/user/projects", nvim_dir.join("cwd")).unwrap();
        write_children(&nvim_dir, &[]);

        let shell_names = vec!["bash".to_string()];
        let helper_names: Vec<String> = vec![];
        let result = resolve_child_process_at(tmp.path(), 1000, &shell_names, &helper_names, 20);
        assert_eq!(
            result,
            Some((
                vec!["nvim".to_string(), "/path/to/file".to_string()],
                "/home/user/projects".to_string()
            ))
        );
    }

    #[test]
    fn resolve_skips_helpers() {
        let tmp = tempfile::tempdir().unwrap();

        let kitty_dir = create_fake_proc_dir(tmp.path(), 1000);
        write_cmdline(&kitty_dir, &["kitty"]);
        write_comm(&kitty_dir, "kitty");
        write_children(&kitty_dir, &[1001]);

        let kitten_dir = create_fake_proc_dir(tmp.path(), 1001);
        write_cmdline(&kitten_dir, &["kitten", "@", "ssh"]);
        write_comm(&kitten_dir, "kitten");
        write_children(&kitten_dir, &[1002]);

        let btop_dir = create_fake_proc_dir(tmp.path(), 1002);
        write_cmdline(&btop_dir, &["btop"]);
        write_comm(&btop_dir, "btop");
        symlink("/home/user", btop_dir.join("cwd")).unwrap();
        write_children(&btop_dir, &[]);

        let shell_names: Vec<String> = vec![];
        let helper_names = vec!["kitten".to_string()];
        let result = resolve_child_process_at(tmp.path(), 1000, &shell_names, &helper_names, 20);
        assert_eq!(
            result,
            Some((vec!["btop".to_string()], "/home/user".to_string()))
        );
    }

    #[test]
    fn resolve_returns_none_when_only_shell() {
        let tmp = tempfile::tempdir().unwrap();

        let kitty_dir = create_fake_proc_dir(tmp.path(), 1000);
        write_cmdline(&kitty_dir, &["kitty"]);
        write_comm(&kitty_dir, "kitty");
        write_children(&kitty_dir, &[1001]);

        let fish_dir = create_fake_proc_dir(tmp.path(), 1001);
        write_cmdline(&fish_dir, &["fish"]);
        write_comm(&fish_dir, "fish");
        write_children(&fish_dir, &[]);

        let shell_names = vec!["fish".to_string()];
        let helper_names: Vec<String> = vec![];
        let result = resolve_child_process_at(tmp.path(), 1000, &shell_names, &helper_names, 20);
        assert!(result.is_none());
    }

    #[test]
    fn resolve_returns_none_when_pid_missing() {
        let tmp = tempfile::tempdir().unwrap();

        let shell_names = vec!["fish".to_string()];
        let helper_names: Vec<String> = vec![];
        let result = resolve_child_process_at(tmp.path(), 9999, &shell_names, &helper_names, 20);
        assert!(result.is_none());
    }

    #[test]
    fn resolve_filters_atexit() {
        let tmp = tempfile::tempdir().unwrap();

        let kitty_dir = create_fake_proc_dir(tmp.path(), 1000);
        write_cmdline(&kitty_dir, &["kitty"]);
        write_comm(&kitty_dir, "kitty");
        write_children(&kitty_dir, &[1001]);

        let fish_dir = create_fake_proc_dir(tmp.path(), 1001);
        write_cmdline(&fish_dir, &["fish"]);
        write_comm(&fish_dir, "fish");
        write_children(&fish_dir, &[]);

        let atexit_stat = "1001 (fish) S 0 0 0 0 -1 1002\n";
        fs::write(fish_dir.join("stat"), atexit_stat).unwrap();

        let atexit_dir = create_fake_proc_dir(tmp.path(), 1002);
        write_cmdline(&atexit_dir, &["__atexit__"]);
        write_comm(&atexit_dir, "__atexit__");
        symlink("/home/user", atexit_dir.join("cwd")).unwrap();

        let shell_names = vec!["fish".to_string()];
        let helper_names: Vec<String> = vec![];
        let result = resolve_child_process_at(tmp.path(), 1000, &shell_names, &helper_names, 20);
        assert!(result.is_none());
    }

    #[test]
    fn resolve_uses_tpgid_fallback() {
        let tmp = tempfile::tempdir().unwrap();

        let kitty_dir = create_fake_proc_dir(tmp.path(), 1000);
        write_cmdline(&kitty_dir, &["kitty"]);
        write_comm(&kitty_dir, "kitty");
        write_children(&kitty_dir, &[1001]);

        let fish_dir = create_fake_proc_dir(tmp.path(), 1001);
        write_cmdline(&fish_dir, &["fish"]);
        write_comm(&fish_dir, "fish");
        write_children(&fish_dir, &[]);

        let stat_content = "1001 (fish) S 0 0 0 0 2000\n";
        fs::write(fish_dir.join("stat"), stat_content).unwrap();

        let btop_dir = create_fake_proc_dir(tmp.path(), 2000);
        write_cmdline(&btop_dir, &["btop"]);
        write_comm(&btop_dir, "btop");
        symlink("/home/user", btop_dir.join("cwd")).unwrap();

        let shell_names = vec!["fish".to_string()];
        let helper_names: Vec<String> = vec![];
        let result = resolve_child_process_at(tmp.path(), 1000, &shell_names, &helper_names, 20);
        assert_eq!(
            result,
            Some((vec!["btop".to_string()], "/home/user".to_string()))
        );
    }

    #[test]
    fn resolve_prefers_foreground_child_over_first_child() {
        let tmp = tempfile::tempdir().unwrap();

        let kitty_dir = create_fake_proc_dir(tmp.path(), 1000);
        write_cmdline(&kitty_dir, &["kitty"]);
        write_comm(&kitty_dir, "kitty");
        write_children(&kitty_dir, &[1001]);

        let fish_dir = create_fake_proc_dir(tmp.path(), 1001);
        write_cmdline(&fish_dir, &["fish"]);
        write_comm(&fish_dir, "fish");
        write_children(&fish_dir, &[1002, 1003]);
        // tpgid (field 8) points to 1003 = the foreground process
        fs::write(fish_dir.join("stat"), "1001 (fish) S 0 0 0 0 1003\n").unwrap();

        let htop_dir = create_fake_proc_dir(tmp.path(), 1002);
        write_cmdline(&htop_dir, &["htop"]);
        write_comm(&htop_dir, "htop");

        let btop_dir = create_fake_proc_dir(tmp.path(), 1003);
        write_cmdline(&btop_dir, &["btop"]);
        write_comm(&btop_dir, "btop");
        symlink("/home/user", btop_dir.join("cwd")).unwrap();

        let shell_names = vec!["fish".to_string()];
        let helper_names: Vec<String> = vec![];
        let result = resolve_child_process_at(tmp.path(), 1000, &shell_names, &helper_names, 20);
        assert_eq!(
            result,
            Some((vec!["btop".to_string()], "/home/user".to_string()))
        );
    }

    #[test]
    fn is_shell_detection() {
        let shells = vec!["fish".to_string(), "bash".to_string(), "-fish".to_string()];
        assert!(is_shell("fish", &shells));
        assert!(is_shell("-fish", &shells));
        assert!(!is_shell("btop", &shells));
        assert!(!is_shell("nvim", &shells));
    }

    #[test]
    fn is_helper_detection() {
        let helpers = vec!["kitten".to_string()];
        assert!(is_helper("kitten", &helpers));
        assert!(!is_helper("btop", &helpers));
    }
}

use std::{fs, os::unix::fs::MetadataExt};
use nix::{
    unistd::{self, Pid},
    sys::signal::{self, Signal},
};
use crate::logging::{LogLevel, log};

pub fn find_processes_by_name(name_pattern: &str, args_pattern: Option<&str>) -> Vec<(Pid, String)> {
    let mut pids = Vec::new();
    let our_pid = std::process::id() as i32;
    
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.filter_map(Result::ok) {
            if !entry.file_name()
                .to_string_lossy()
                .chars()
                .all(|c| c.is_numeric())
            {
                continue;
            }

            let pid_str = entry.file_name().to_string_lossy().to_string();
            let pid = match pid_str.parse::<i32>() {
                Ok(pid) => pid,
                Err(_) => continue,
            };

            if pid == our_pid {
                continue;
            }

            if let Ok(metadata) = entry.metadata() {
                if metadata.uid() != unistd::getuid().as_raw() {
                    continue;
                }
            } else {
                continue;
            }

            let mut found = false;
            let mut process_name = String::new();

            // Check /proc/[pid]/comm
            if let Ok(comm) = fs::read_to_string(entry.path().join("comm")) {
                let comm = comm.trim();
                if comm.contains(name_pattern) {
                    if let Some(args_pattern) = args_pattern {
                        if let Ok(cmdline) = fs::read_to_string(entry.path().join("cmdline")) {
                            if cmdline.contains(args_pattern) {
                                found = true;
                                process_name = comm.to_string();
                            }
                        }
                    } else {
                        found = true;
                        process_name = comm.to_string();
                    }
                }
            }

            if found {
                pids.push((Pid::from_raw(pid), process_name));
            }
        }
    }
    pids
}

pub fn kill_processes(processes: &[(Pid, String)], force: bool) -> usize {
    let mut killed = 0;
    for (pid, name) in processes {
        log(LogLevel::Warning, &format!("Sending {} to {} (PID: {})", 
            if force { "SIGKILL" } else { "SIGTERM" }, 
            name, pid));

        let signal = if force { Signal::SIGKILL } else { Signal::SIGTERM };
        if signal::kill(*pid, signal).is_ok() {
            killed += 1;
        }
    }
    killed
}

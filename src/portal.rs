use std::{
    fs::File,
    io::{BufRead, BufReader},
    os::unix::io::{FromRawFd, RawFd},
    path::Path,
    process,
    sync::mpsc,
    thread,
    time::Duration,
};
use nix::{
    unistd::{dup2, ForkResult, fork, setsid},
};
use crate::{
    logging::{LogLevel, log, format_pid},
    process::{find_processes_by_name, kill_processes},
};

pub const HYPR_PORTAL: &str = "/usr/lib/xdg-desktop-portal-hyprland";
pub const XDG_PORTAL: &str = "/usr/lib/xdg-desktop-portal";

pub fn check_portal_paths() {
    log(LogLevel::Info, "Checking portal binaries...");
    
    let paths = [
        ("Hyprland Portal", HYPR_PORTAL),
        ("XDG Portal", XDG_PORTAL),
    ];

    for (name, path) in paths {
        if Path::new(path).exists() {
            log(LogLevel::Success, &format!("Found {} at {}", name, path));
        } else {
            log(LogLevel::Error, &format!("{} not found at {}", name, path));
        }
    }
}

pub fn find_portal_processes() -> Vec<(nix::unistd::Pid, String)> {
    find_processes_by_name("xdg-desktop-portal", None)
}

pub fn kill_portal_processes() -> usize {
    log(LogLevel::Info, "Looking for portal processes...");
    
    let processes = find_portal_processes();
    if processes.is_empty() {
        log(LogLevel::Warning, "No portal processes found");
        return 0;
    }

    log(LogLevel::Info, &format!("Found {} portal process(es)", processes.len()));
    for (pid, name) in &processes {
        log(LogLevel::Info, &format!("→ {} (PID: {})", name, pid));
    }

    let killed = kill_processes(&processes, false);
    
    // Check for remaining processes and force kill
    thread::sleep(Duration::from_millis(100));
    let remaining = find_portal_processes();
    if !remaining.is_empty() {
        kill_processes(&remaining, true);
    }

    if killed > 0 {
        log(LogLevel::Success, &format!("Successfully terminated {} portal process(es)", killed));
    }
    killed
}

pub fn spawn_portal(path: &str, name: &str) -> nix::Result<()> {
    if !Path::new(path).exists() {
        log(LogLevel::Error, &format!("Portal binary not found: {}", path));
        return Ok(());
    }

    log(LogLevel::Info, &format!("Starting {}...", name));
    
    let (reader_rx, writer_tx) = nix::unistd::pipe()?;
    let name = name.to_string();
    let name_clone = name.clone();
    
    match unsafe { fork()? } {
        ForkResult::Parent { child } => {
            nix::unistd::close(writer_tx)?;
            let (tx, rx) = mpsc::channel();
            
            thread::spawn(move || {
                let file = unsafe { File::from_raw_fd(reader_rx) };
                let reader = BufReader::new(file);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        tx.send(line).ok();
                    }
                }
            });

            thread::spawn(move || {
                while let Ok(line) = rx.recv() {
                    log(LogLevel::Process, &format!("[{}] {}", name_clone, line));
                }
            });

            log(LogLevel::Success, &format!("Started {} {}", name, format_pid(child)));
            Ok(())
        },
        ForkResult::Child => {
            setsid()?;
            
            for fd in 0..=2 {
                if fd != 1 && fd != 2 {
                    let _ = nix::unistd::close(fd as RawFd);
                } else {
                    let _ = dup2(writer_tx, fd as RawFd);
                }
            }
            let _ = nix::unistd::close(reader_rx);
            
            let err = process::Command::new(path)
                .arg("-v")
                .spawn()
                .expect("failed to execute portal")
                .wait();
            
            process::exit(match err {
                Ok(status) => status.code().unwrap_or(1),
                Err(_) => 1,
            });
        }
    }
}

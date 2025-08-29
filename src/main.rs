use nix::{
    sys::signal::{self, Signal},
    unistd::{dup2, ForkResult, Pid, fork, setsid},
};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    os::{
        unix::fs::MetadataExt,
        unix::io::{FromRawFd, RawFd},
    },
    path::Path,
    process,
    sync::mpsc,
    thread,
    time::Duration,
};
use chrono::Local;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";

#[derive(Debug)]
enum LogLevel {
    Info,
    Success,
    Warning,
    Error,
    Process,
    Action,
}

impl LogLevel {
    fn style(&self) -> (&'static str, &'static str, &'static str) { // (prefix, color, symbol)
        match self {
            LogLevel::Info => ("INFO", "\x1b[94m", "ℹ"),    // Bright Blue
            LogLevel::Success => ("OK", "\x1b[92m", "✓"),   // Bright Green
            LogLevel::Warning => ("WARN", "\x1b[93m", "⚠"),  // Bright Yellow
            LogLevel::Error => ("ERROR", "\x1b[91m", "✗"),   // Bright Red
            LogLevel::Process => ("LOG", "\x1b[96m", "→"),   // Bright Cyan
            LogLevel::Action => ("RUN", "\x1b[95m", "⚡"),   // Bright Magenta
        }
    }
}

const HYPR_PORTAL: &str = "/usr/lib/xdg-desktop-portal-hyprland";
const XDG_PORTAL: &str = "/usr/lib/xdg-desktop-portal";

fn format_pid(pid: impl std::fmt::Display) -> String {
    format!("{}({}PID: {}{}{}", DIM, RESET, BOLD, pid, DIM)
}

fn log(level: LogLevel, msg: &str) {
    let timestamp = Local::now().format("%H:%M:%S%.3f");
    let (prefix, color, symbol) = level.style();
    
    eprintln!(
        "{color}{symbol} {}{DIM}[{timestamp}]{RESET} {color}{BOLD}{prefix}{RESET} {msg}",
        DIM,
    );
}

fn find_portal_processes() -> Vec<(Pid, String)> {
    let mut pids = Vec::new();
    
    // Get our own PID to exclude it
    let our_pid = std::process::id() as i32;
    
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.filter_map(Result::ok) {
            // Only look at numeric directory names (PIDs)
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

            // Skip our own process
            if pid == our_pid {
                continue;
            }

            // Skip if we don't own the process
            if let Ok(metadata) = entry.metadata() {
                if metadata.uid() != nix::unistd::getuid().as_raw() {
                    continue;
                }
            } else {
                continue;
            }

            // Check both comm and cmdline
            let mut is_portal = false;
            let mut process_name = String::new();

            // Check /proc/[pid]/comm
            if let Ok(comm) = fs::read_to_string(entry.path().join("comm")) {
                let comm = comm.trim();
                if comm.starts_with("xdg-desktop-portal") {
                    is_portal = true;
                    process_name = comm.to_string();
                }
            }

            // Also check /proc/[pid]/cmdline
            if !is_portal {
                if let Ok(cmdline) = fs::read_to_string(entry.path().join("cmdline")) {
                    let args: Vec<&str> = cmdline.split('\0').collect();
                    if let Some(cmd) = args.first() {
                        if cmd.contains("xdg-desktop-portal") {
                            is_portal = true;
                            process_name = cmd.split('/').last()
                                .unwrap_or("unknown-portal")
                                .to_string();
                        }
                    }
                }
            }

            // Also check /proc/[pid]/exe symlink
            if !is_portal {
                if let Ok(exe) = fs::read_link(entry.path().join("exe")) {
                    if let Some(exe_str) = exe.to_str() {
                        if exe_str.contains("xdg-desktop-portal") {
                            is_portal = true;
                            process_name = exe.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown-portal")
                                .to_string();
                        }
                    }
                }
            }

            if is_portal {
                pids.push((Pid::from_raw(pid), process_name));
            }
        }
    }
    pids
}

fn find_dbus_processes() -> Vec<(Pid, String)> {
    let mut pids = Vec::new();
    
    log(LogLevel::Action, "Scanning system processes");
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.filter_map(Result::ok) {
            if !entry.file_name().to_string_lossy().chars().all(|c| c.is_numeric()) {
                continue;
            }

            let pid_str = entry.file_name().to_string_lossy().to_string();
            let pid = match pid_str.parse::<i32>() {
                Ok(pid) => pid,
                Err(_) => continue,
            };

            // Check if this is a dbus-daemon process
            if let Ok(comm) = fs::read_to_string(entry.path().join("comm")) {
                let comm = comm.trim();
                if comm == "dbus-daemon" {
                    if let Ok(cmdline) = fs::read_to_string(entry.path().join("cmdline")) {
                        if cmdline.contains("--session") {
                            pids.push((Pid::from_raw(pid), "dbus-daemon".to_string()));
                        }
                    }
                }
            }
        }
    }
    pids
}

fn kill_portal_processes() -> usize {
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

    let mut killed = 0;
    for (pid, name) in processes {
        log(LogLevel::Warning, &format!("Sending SIGTERM to {} (PID: {})", name, pid));
        if signal::kill(pid, Signal::SIGTERM).is_ok() {
            killed += 1;
            thread::sleep(Duration::from_millis(100));
            
            // Check if process still exists
            if find_portal_processes().iter().any(|(p, _)| p == &pid) {
                log(LogLevel::Error, &format!("Process {} still alive, sending SIGKILL", pid));
                let _ = signal::kill(pid, Signal::SIGKILL);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }

    if killed > 0 {
        log(LogLevel::Success, &format!("Successfully terminated {} portal process(es)", killed));
    }
    killed
}

fn spawn_portal(path: &str, name: &str) -> nix::Result<()> {
    if !Path::new(path).exists() {
        log(LogLevel::Error, &format!("Portal binary not found: {}", path));
        return Ok(());
    }

    log(LogLevel::Info, &format!("Starting {}...", name));
    
    // Create a pipe for stdout/stderr
    let (reader_rx, writer_tx) = nix::unistd::pipe()?;
    
    // Clone name for use in threads
    let name = name.to_string();
    let name_clone = name.clone();
    
    match unsafe { fork()? } {
        ForkResult::Parent { child } => {
            // Close write end in parent
            nix::unistd::close(writer_tx)?;
            
            // Set up a channel to receive output from the reader thread
            let (tx, rx) = mpsc::channel();
            
            // Spawn a thread to read the process output
            thread::spawn(move || {
                let file = unsafe { File::from_raw_fd(reader_rx) };
                let reader = BufReader::new(file);
                for line in reader.lines() {
                    if let Ok(line) = line {
                        tx.send(line).ok();
                    }
                }
            });

            // Log the process output in a non-blocking way
            thread::spawn(move || {
                while let Ok(line) = rx.recv() {
                    log(LogLevel::Process, &format!("[{}] {}", name_clone, line));
                }
            });

            log(LogLevel::Success, &format!("Started {} {}", name, format_pid(child)));
            Ok(())
        },
        ForkResult::Child => {
            // Create new session
            setsid()?;
            
            // Redirect stdout and stderr to the pipe
            for fd in 0..=2 {
                if fd != 1 && fd != 2 {
                    let _ = nix::unistd::close(fd as RawFd);
                } else {
                    let _ = dup2(writer_tx, fd as RawFd);
                }
            }
            let _ = nix::unistd::close(reader_rx);
            
            // Execute the portal
            let err = process::Command::new(path)
                .arg("-v")
                .spawn()
                .expect("failed to execute portal")
                .wait();
            
            // Exit the child process
            process::exit(match err {
                Ok(status) => status.code().unwrap_or(1),
                Err(_) => 1,
            });
        }
    }
}

// Function to restart DBus if needed
fn restart_dbus() {
    log(LogLevel::Info, "Checking DBus session...");
    let dbus_processes = find_dbus_processes();
    
    if !dbus_processes.is_empty() {
        log(LogLevel::Warning, "Restarting DBus session...");
        for (pid, _) in dbus_processes {
            let _ = signal::kill(pid, Signal::SIGTERM);
        }
        thread::sleep(Duration::from_millis(500));
        
        // Wait for dbus to be completely gone
        for _ in 0..10 {
            if find_dbus_processes().is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        
        // Start a new dbus-daemon
        if let Ok(child) = process::Command::new("dbus-daemon")
            .args(["--session", "--address=unix:runtime=yes"])
            .spawn()
        {
            thread::sleep(Duration::from_millis(500));
            log(LogLevel::Success, &format!("Started new DBus session (PID: {})", child.id()));
        } else {
            log(LogLevel::Error, "Failed to start new DBus session");
        }
    }
}

fn main() -> nix::Result<()> {
    log(LogLevel::Info, "Portal reset started");
    log(LogLevel::Info, "Waiting 1 second before killing existing portals...");
    thread::sleep(Duration::from_secs(1));

    // Kill all portal processes
    let killed = kill_portal_processes();
    if killed > 0 {
        log(LogLevel::Info, "Waiting 0.5 seconds for processes to clean up...");
        thread::sleep(Duration::from_millis(500));
    }
    
    // Check and restart DBus if needed
    restart_dbus();

    log(LogLevel::Info, "Checking DBus session...");
    let dbus_processes = find_dbus_processes();
    
    if !dbus_processes.is_empty() {
        log(LogLevel::Warning, "Restarting DBus session...");
        for (pid, _) in dbus_processes {
            let _ = signal::kill(pid, Signal::SIGTERM);
        }
        thread::sleep(Duration::from_millis(500));
        
        // Wait for dbus to be completely gone
        for _ in 0..10 {
            if find_dbus_processes().is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        
        // Start a new dbus-daemon
        if let Ok(child) = process::Command::new("dbus-daemon")
            .args(["--session", "--address=unix:runtime=yes"])
            .spawn()
        {
            thread::sleep(Duration::from_millis(500));
            log(LogLevel::Success, &format!("Started new DBus session (PID: {})", child.id()));
        } else {
            log(LogLevel::Error, "Failed to start new DBus session");
        }
    }

    // Start Hyprland portal with retries
    let mut retry_count = 0;
    let max_retries = 3;
    
    loop {
        spawn_portal(HYPR_PORTAL, "Hyprland portal")?;
        log(LogLevel::Info, "Waiting for Hyprland portal to initialize...");
        
        // Wait and check for successful initialization
        let mut success = true;
        for _ in 0..20 {
            thread::sleep(Duration::from_millis(100));
            if find_portal_processes().is_empty() {
                success = false;
                break;
            }
        }

        if success {
            break;
        }

        retry_count += 1;
        if retry_count >= max_retries {
            log(LogLevel::Error, "Failed to start Hyprland portal after multiple attempts");
            return Ok(());
        }

        log(LogLevel::Warning, &format!("Hyprland portal failed to start, retrying ({}/{})", retry_count, max_retries));
        // Kill any remaining processes and restart DBus
        kill_portal_processes();
        restart_dbus();
    }

    // Extra wait to ensure Hyprland portal is fully initialized
    thread::sleep(Duration::from_secs(2));

    // Start XDG portal
    spawn_portal(XDG_PORTAL, "XDG portal")?;
    
    // Wait to verify XDG portal starts successfully
    let mut success = false;
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(100));
        if !find_portal_processes().iter().any(|(_, name)| name == "xdg-desktop-portal") {
            log(LogLevel::Warning, "XDG portal failed to start, checking Hyprland portal...");
            // If XDG portal failed, check if Hyprland portal is still running
            if find_portal_processes().iter().any(|(_, name)| name == "xdg-desktop-portal-hyprland") {
                log(LogLevel::Info, "Hyprland portal still running, continuing...");
                success = true;
            }
            break;
        } else {
            success = true;
        }
    }

    if success {
        log(LogLevel::Success, "Portal reset completed successfully");
    } else {
        log(LogLevel::Error, "Portal reset completed with errors");
    }
    Ok(())
}

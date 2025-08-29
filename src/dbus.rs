use std::{process, thread, time::Duration};
use crate::{
    logging::{LogLevel, log},
    process::{find_processes_by_name, kill_processes},
};

pub fn find_dbus_processes() -> Vec<(nix::unistd::Pid, String)> {
    find_processes_by_name("dbus-daemon", Some("--session"))
}

pub fn restart_dbus() {
    log(LogLevel::Info, "Checking DBus session...");
    let dbus_processes = find_dbus_processes();
    
    if !dbus_processes.is_empty() {
        log(LogLevel::Warning, "Restarting DBus session...");
        kill_processes(&dbus_processes, false);
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

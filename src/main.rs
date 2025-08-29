mod logging;
mod process;
mod portal;
mod dbus;

use std::{thread, time::Duration};
use logging::{LogLevel, log};
use portal::{HYPR_PORTAL, XDG_PORTAL, kill_portal_processes, spawn_portal, find_portal_processes};
use dbus::restart_dbus;

fn main() -> nix::Result<()> {
    log(LogLevel::Info, "Portal reset started");
    portal::check_portal_paths();
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

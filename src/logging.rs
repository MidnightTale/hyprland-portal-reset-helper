use chrono::Local;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";

#[derive(Debug)]
pub enum LogLevel {
    Info,
    Success,
    Warning,
    Error,
    Process,
    Action,
}

impl LogLevel {
    pub fn style(&self) -> (&'static str, &'static str, &'static str) { // (prefix, color, symbol)
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

pub fn format_pid(pid: impl std::fmt::Display) -> String {
    format!("{}({}PID: {}{}{}", DIM, RESET, BOLD, pid, DIM)
}

pub fn log(level: LogLevel, msg: &str) {
    let timestamp = Local::now().format("%H:%M:%S%.3f");
    let (prefix, color, symbol) = level.style();
    
    eprintln!(
        "{color}{symbol} {}{DIM}[{timestamp}]{RESET} {color}{BOLD}{prefix}{RESET} {msg}",
        DIM,
    );
}

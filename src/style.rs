use std::io::IsTerminal;
use std::sync::OnceLock;

static USE_COLOR: OnceLock<bool> = OnceLock::new();

pub fn init() {
    let enabled = if std::env::var_os("NO_COLOR").is_some() {
        false
    } else if std::env::var_os("FORCE_COLOR").is_some() {
        true
    } else {
        std::io::stdout().is_terminal()
    };
    let _ = USE_COLOR.set(enabled);
}

fn enabled() -> bool {
    *USE_COLOR.get().unwrap_or(&false)
}

fn wrap(code: &str, s: &str) -> String {
    if enabled() {
        format!("\x1b[{}m{}\x1b[0m", code, s)
    } else {
        s.to_string()
    }
}

pub fn bold(s: &str) -> String {
    wrap("1", s)
}
pub fn dim(s: &str) -> String {
    wrap("2", s)
}
pub fn green(s: &str) -> String {
    wrap("38;5;72", s)
}
pub fn amber(s: &str) -> String {
    wrap("38;5;179", s)
}
pub fn red(s: &str) -> String {
    wrap("38;5;167", s)
}
pub fn violet(s: &str) -> String {
    wrap("38;5;103", s)
}
pub fn cyan(s: &str) -> String {
    wrap("38;5;73", s)
}

//! Dependency-free terminal styling for command output.
//!
//! Color is emitted only when the destination is an interactive terminal (or
//! when explicitly forced) and is disabled by `NO_COLOR`, so piped output and
//! captured agent output stay plain. Values that come from sources are
//! sanitized before they reach these helpers.

use std::io::IsTerminal;
use std::sync::OnceLock;

const RESET: &str = "\x1b[0m";

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

/// Explicit user intent from the environment, if any.
fn override_choice() -> Option<bool> {
    if let Ok(value) = std::env::var("SKILLS_COLOR") {
        match value.trim().to_ascii_lowercase().as_str() {
            "always" | "force" | "yes" | "true" | "1" => return Some(true),
            "never" | "none" | "no" | "false" | "0" => return Some(false),
            _ => {}
        }
    }
    if std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()) {
        return Some(false);
    }
    if std::env::var_os("CLICOLOR_FORCE").is_some_and(|value| value != "0") {
        return Some(true);
    }
    if std::env::var("CLICOLOR").is_ok_and(|value| value == "0") {
        return Some(false);
    }
    None
}

fn enabled(stream: Stream) -> bool {
    static STDOUT: OnceLock<bool> = OnceLock::new();
    static STDERR: OnceLock<bool> = OnceLock::new();
    let slot = match stream {
        Stream::Stdout => &STDOUT,
        Stream::Stderr => &STDERR,
    };
    *slot.get_or_init(|| match override_choice() {
        Some(choice) => choice,
        None => match stream {
            Stream::Stdout => std::io::stdout().is_terminal(),
            Stream::Stderr => std::io::stderr().is_terminal(),
        },
    })
}

fn paint(stream: Stream, code: &str, text: &str) -> String {
    if text.is_empty() || !enabled(stream) {
        return text.to_string();
    }
    format!("\x1b[{code}m{text}{RESET}")
}

/// A section heading on stdout (bold cyan).
pub fn heading(text: &str) -> String {
    paint(Stream::Stdout, "1;36", text)
}

/// A field label or secondary identifier on stdout (cyan).
pub fn label(text: &str) -> String {
    paint(Stream::Stdout, "36", text)
}

/// A filesystem path or revision on stdout (dim).
pub fn path(text: &str) -> String {
    paint(Stream::Stdout, "2", text)
}

/// A successful outcome marker on stdout (green).
pub fn ok(text: &str) -> String {
    paint(Stream::Stdout, "32", text)
}

/// A warning marker on stdout (yellow).
pub fn warn_stdout(text: &str) -> String {
    paint(Stream::Stdout, "33", text)
}

/// Bold text on stdout.
pub fn bold(text: &str) -> String {
    paint(Stream::Stdout, "1", text)
}

/// The literal `warning` prefix on stderr (yellow).
pub fn warning_label() -> String {
    paint(Stream::Stderr, "1;33", "warning")
}

/// The literal `error` prefix on stderr (red).
pub fn error_label() -> String {
    paint(Stream::Stderr, "1;31", "error")
}

/// A diagnostic level rendered on stdout (yellow for warnings, red for errors).
pub fn level(level: &str) -> String {
    match level {
        "error" => paint(Stream::Stdout, "1;31", level),
        _ => paint(Stream::Stdout, "1;33", level),
    }
}

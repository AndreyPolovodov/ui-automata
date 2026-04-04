//! `ui-workflow-check` — validate workflow YAML files without running them.
//!
//! Usage:
//!   ui-workflow-check <workflow.yml> [<workflow2.yml> ...]
//!
//! Exits 0 if all files are valid, 1 if any diagnostics are found.

use ui_automata::lint::{self, LintDiag};

fn main() {
    // Suppress the console window when run as a subprocess (stdout is piped).
    #[cfg(windows)]
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        unsafe extern "system" {
            fn FreeConsole() -> i32;
        }
        unsafe {
            FreeConsole();
        }
    }

    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("Usage: ui-workflow-check <workflow.yml> [<workflow2.yml> ...]");
        std::process::exit(2);
    }

    let multi = args.len() > 1;
    let mut any_errors = false;

    for path in &args {
        let raw = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {e}");
                eprintln!("  --> {path}");
                any_errors = true;
                continue;
            }
        };

        let diags = lint::lint(&raw);

        if diags.is_empty() {
            if multi {
                println!("{path}: ok");
            }
        } else {
            any_errors = true;
            let lines: Vec<&str> = raw.lines().collect();
            for d in &diags {
                render_diag(d, path, &lines);
            }
            let n = diags.len();
            eprintln!("aborting due to {n} error{}", if n == 1 { "" } else { "s" });
        }
    }

    if any_errors {
        std::process::exit(1);
    }
}

fn render_diag(d: &LintDiag, file: &str, lines: &[&str]) {
    // Header
    eprintln!("error: {}", d.message);

    // Location arrow
    match (d.line, d.col) {
        (Some(l), Some(c)) => eprintln!("  --> {file}:{l}:{c}"),
        (Some(l), None) => eprintln!("  --> {file}:{l}"),
        _ => eprintln!("  --> {file}"),
    }

    // Source context with underline
    if let Some(line_num) = d.line {
        let source = lines.get(line_num.saturating_sub(1)).copied().unwrap_or("");
        let num_str = line_num.to_string();
        let pad = " ".repeat(num_str.len());

        eprintln!("{pad}  |");
        eprintln!("{num_str}  | {source}");

        // Build underline: spaces up to col, then carets
        if let Some(col) = d.col {
            let indent = col.saturating_sub(1);
            let caret_len = d
                .end_col
                .and_then(|e| e.checked_sub(col))
                .filter(|&n| n > 0)
                .unwrap_or(1);
            let underline = format!("{}{}", " ".repeat(indent), "^".repeat(caret_len));
            if d.path.is_empty() {
                eprintln!("{pad}  | {underline}");
            } else {
                eprintln!("{pad}  | {underline} {}", d.path);
            }
        }
    } else if !d.path.is_empty() {
        eprintln!("      = note: {}", d.path);
    }

    eprintln!();
}

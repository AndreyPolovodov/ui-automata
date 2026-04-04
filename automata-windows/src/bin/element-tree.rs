/// Walk the full UIA element tree of a window and print it as YAML.
/// In --interactive mode, drops into a REPL for testing selectors against a snapshot.

#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
#[derive(clap::Parser)]
#[command(about = "Walk the UIA element tree of a window")]
struct Args {
    /// Window handle (hex: 0x1a2b3c or decimal).
    hwnd: String,

    /// Drop into an interactive selector REPL against a snapshot of the tree.
    #[arg(short, long)]
    interactive: bool,
}

#[cfg(target_os = "windows")]
fn main() {
    use clap::Parser;
    let args = Args::parse();

    let hwnd = parse_hwnd(&args.hwnd);

    automata_windows::init_logging(None);
    automata_windows::init_com();

    if args.interactive {
        run_interactive(hwnd);
    } else {
        let tree = match automata_windows::build_element_tree(
            None,
            None,
            None,
            None,
            Some(hwnd),
            usize::MAX,
            None,
        ) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Failed: {e}");
                std::process::exit(1);
            }
        };
        let yaml = serde_yaml::to_string(&tree).expect("Failed to serialize to YAML");
        print!("{yaml}");
    }
}

#[cfg(target_os = "windows")]
fn parse_hwnd(s: &str) -> u64 {
    let result = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else {
        s.parse::<u64>()
    };
    result.unwrap_or_else(|_| {
        eprintln!("Invalid hwnd: {s:?}");
        std::process::exit(1);
    })
}

#[cfg(target_os = "windows")]
fn run_interactive(hwnd: u64) {
    use std::io::{BufRead, Write};

    eprint!("constructing element tree ...");

    let root = match automata_windows::snapshot_tree(hwnd) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("\nFailed: {e}");
            std::process::exit(1);
        }
    };

    eprintln!(" done.");

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();

    loop {
        print!("$ ");
        stdout.lock().flush().ok();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) | Err(_) => break,
            _ => {}
        }

        let selector = line.trim();
        if selector.is_empty() {
            continue;
        }
        if selector == "quit" || selector == "exit" {
            break;
        }

        let path = match ui_automata::SelectorPath::parse(selector) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("parse error: {e}");
                continue;
            }
        };

        let matches = path.find_all(&root);
        if matches.is_empty() {
            eprintln!("(no matches)");
            continue;
        }

        for m in &matches {
            let mut out = format!("[role={}]", m.role);
            if !m.name.is_empty() {
                out.push_str(&format!("[name={:?}]", m.name));
            }
            if let Some(id) = &m.automation_id {
                out.push_str(&format!("[id={id}]"));
            }
            let value = m.text.as_deref().unwrap_or("");
            if !value.is_empty() {
                out.push_str(&format!(" value={value:?}"));
            }
            out.push_str(&format!(" rect=({},{},{},{})", m.x, m.y, m.width, m.height));
            println!("{out}");
        }
    }
}

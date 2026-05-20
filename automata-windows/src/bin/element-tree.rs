/// Walk the full UIA element tree of a window and print it as YAML.
/// In --interactive mode, drops into a REPL for testing selectors against a snapshot.

#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
#[derive(clap::Parser)]
#[command(about = "Walk the UIA element tree of a window")]
struct Args {
    /// Window handle (hex: 0x1a2b3c or decimal). Takes priority over --process.
    #[arg(long)]
    hwnd: Option<String>,

    /// Process name without .exe (e.g. Integral.Shell). Used when --hwnd is not supplied.
    #[arg(long, short)]
    process: Option<String>,

    /// CSS-like selector to filter output (e.g. ">> [role=pane][name~=NcWayMeasurer]").
    /// Prints matching elements with role/name/id/bounds.
    #[arg(long, short)]
    selector: Option<String>,

    /// When --selector is given, also print children up to this many levels deep (default: 0).
    #[arg(long, short, default_value = "0")]
    depth: usize,

    /// Drop into an interactive selector REPL against a snapshot of the tree.
    #[arg(short, long)]
    interactive: bool,
}

#[cfg(target_os = "windows")]
fn main() {
    use clap::Parser;
    let args = Args::parse();

    automata_windows::init_logging(None);
    automata_windows::init_com();

    let hwnd = resolve_hwnd(&args);

    if args.interactive {
        run_interactive(hwnd);
    } else if let Some(selector) = &args.selector {
        run_selector(hwnd, selector, args.depth);
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
fn resolve_hwnd(args: &Args) -> u64 {
    if let Some(hwnd_str) = &args.hwnd {
        return parse_hwnd(hwnd_str);
    }
    if let Some(process) = &args.process {
        let windows = automata_windows::find_windows(process).unwrap_or_else(|e| {
            eprintln!("Failed to list windows: {e}");
            std::process::exit(1);
        });
        if windows.is_empty() {
            eprintln!("No windows found for process {:?}", process);
            std::process::exit(1);
        }
        return windows[0].hwnd;
    }
    eprintln!("Either hwnd or --process is required");
    std::process::exit(1);
}

#[cfg(target_os = "windows")]
fn run_selector(hwnd: u64, selector: &str, depth: usize) {
    let root = match automata_windows::snapshot_tree(hwnd) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to build tree: {e}");
            std::process::exit(1);
        }
    };

    let path = match ui_automata::SelectorPath::parse(selector) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Selector parse error: {e}");
            std::process::exit(1);
        }
    };

    let matches = path.find_all(&root);
    if matches.is_empty() {
        eprintln!("(no matches)");
        std::process::exit(1);
    }

    for m in &matches {
        print_node(m, 0, depth);
    }
}

#[cfg(target_os = "windows")]
fn print_node(node: &automata_windows::ElementNode, indent: usize, remaining_depth: usize) {
    let pad = "  ".repeat(indent);
    let mut out = format!("{pad}[role={}]", node.role);
    if !node.name.is_empty() {
        out.push_str(&format!("[name={:?}]", node.name));
    }
    if let Some(id) = &node.automation_id {
        out.push_str(&format!("[id={id}]"));
    }
    if let Some(cls) = &node.class_name {
        out.push_str(&format!("[class={cls}]"));
    }
    let value = node.text.as_deref().unwrap_or("");
    if !value.is_empty() {
        out.push_str(&format!(" value={value:?}"));
    }
    if let Some(ts) = node.toggle_state {
        out.push_str(&format!(" toggle_state={ts}"));
    }
    out.push_str(&format!(" rect=({},{},{},{})", node.x, node.y, node.width, node.height));
    println!("{out}");

    if remaining_depth > 0 {
        for child in &node.children {
            print_node(child, indent + 1, remaining_depth - 1);
        }
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
            if let Some(cls) = &m.class_name {
                out.push_str(&format!("[class={cls}]"));
            }
            let value = m.text.as_deref().unwrap_or("");
            if !value.is_empty() {
                out.push_str(&format!(" value={value:?}"));
            }
            if let Some(ts) = m.toggle_state {
                out.push_str(&format!(" toggle_state={ts}"));
            }
            out.push_str(&format!(" rect=({},{},{},{})", m.x, m.y, m.width, m.height));
            println!("{out}");
        }
    }
}

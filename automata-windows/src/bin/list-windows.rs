/// List all top-level windows.
///
/// Usage:  list-windows

#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
fn main() {
    automata_windows::init_logging(None);
    automata_windows::init_com();

    match automata_windows::application_windows() {
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
        Ok(windows) => {
            for (i, w) in windows.iter().enumerate() {
                println!("--- Window #{} ---", i + 1);
                println!("  hwnd    : {:#x}", w.hwnd);
                println!("  process : {} (pid {})", w.process_name, w.pid);
                println!("  title   : {}", w.title);
                println!("  type    : {}", w.control_type);
                println!("  class   : {}", w.class);
                println!("  id      : {}", w.automation_id);
                println!(
                    "  bounds  : x={} y={} w={} h={}",
                    w.x, w.y, w.width, w.height
                );
            }
        }
    }
}

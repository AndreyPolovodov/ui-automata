pub type Result<T> = anyhow::Result<T>;

/// Window state action for [`set_window_state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    Minimize,
    Maximize,
    Restore,
    Close,
}

#[cfg(target_os = "windows")]
mod browser;
mod clipboard;
mod desktop;
mod dialog;
#[cfg(target_os = "windows")]
mod element;
#[cfg(not(target_os = "windows"))]
#[path = "element_stub.rs"]
mod element;
mod element_info;
mod error;
mod input;
mod locator;
mod mouse;
mod mouse_hook;
mod overlay;
mod process;
mod selector;
mod task_view;
mod taskbar;
mod uia_probe;
mod util;
mod window;

#[cfg(target_os = "windows")]
mod element_tree;
#[cfg(target_os = "windows")]
mod windows_info;

pub use clipboard::*;
pub use desktop::*;
pub use dialog::*;
pub use element::*;
pub use element_info::*;
pub use error::*;
pub use input::*;
pub use locator::*;
pub use mouse::*;
pub use mouse_hook::*;
pub use overlay::*;
pub use process::*;
pub use selector::*;
pub use task_view::*;
pub use taskbar::*;
pub use uia_probe::*;
pub use window::*;

#[cfg(target_os = "windows")]
pub use element_tree::*;
#[cfg(target_os = "windows")]
pub use windows_info::*;

/// Run a UI-Automata YAML workflow from an in-memory string.
///
/// `params` is the CLI override map (snake_case keys).
#[cfg(target_os = "windows")]
pub fn run_workflow_str(
    yaml: &str,
    params: &std::collections::HashMap<String, String>,
) -> Result<()> {
    let workflow =
        ui_automata::yaml::WorkflowFile::load_from_str(yaml, params).map_err(anyhow::Error::msg)?;
    let desktop = Desktop::new();
    let mut executor = ui_automata::Executor::new(desktop);
    log::info!("running workflow '{}' ...", workflow.name);
    workflow
        .run(&mut executor, None, None)
        .map(|_| ())
        .map_err(anyhow::Error::msg)
}

/// Workspace crates that get DEBUG level in the log file; everything else is INFO.
const OWN_CRATES: &[&str] = &["automata_browser", "automata_windows", "ui_automata"];

/// Build a file dispatch: INFO for third-party crates, DEBUG for workspace crates.
/// Returns `None` if the file can't be opened.
fn make_file_dispatch(log_path: &std::path::Path) -> Option<fern::Dispatch> {
    match fern::log_file(log_path) {
        Ok(file) => {
            let mut dispatch = fern::Dispatch::new()
                .level(log::LevelFilter::Info)
                .format(|out, message, record| {
                    out.finish(format_args!(
                        "[{}] [{}] {}",
                        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ"),
                        record.level(),
                        message
                    ))
                })
                .chain(file);
            for krate in OWN_CRATES {
                dispatch = dispatch.level_for(*krate, log::LevelFilter::Debug);
            }
            Some(dispatch)
        }
        Err(e) => {
            eprintln!("could not open log file {}: {e}", log_path.display());
            None
        }
    }
}

/// File-only logger — no stdout output. Used in pipe mode (spawned by automata-client).
///
/// Call once at startup before any `log::` calls.
pub fn init_logging_file_only(log_path: &std::path::Path) {
    let mut dispatch = fern::Dispatch::new();
    if let Some(file_dispatch) = make_file_dispatch(log_path) {
        dispatch = dispatch.chain(file_dispatch);
    }
    if let Err(e) = dispatch.apply() {
        eprintln!("failed to initialise logging: {e}");
    }
}

/// Send `CTRL_C_EVENT` to a process by PID. Returns `true` on success.
///
/// The target process must be in the same console process group (the default
/// when spawned via `std::process::Command` without `CREATE_NEW_PROCESS_GROUP`).
#[cfg(target_os = "windows")]
pub fn send_ctrl_c(pid: u32) -> bool {
    use windows::Win32::System::Console::{CTRL_C_EVENT, GenerateConsoleCtrlEvent};
    unsafe { GenerateConsoleCtrlEvent(CTRL_C_EVENT, pid).is_ok() }
}

#[cfg(not(target_os = "windows"))]
pub fn send_ctrl_c(_pid: u32) -> bool {
    false
}

/// Initialise logging: coloured stdout (Info for everything, override with `RUST_LOG`) and optional
/// file (Info for third-party crates, Debug for workspace crates).
///
/// Call once at startup before any `log::` calls.
pub fn init_logging(log_path: Option<&std::path::Path>) {
    let stdout_level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(log::LevelFilter::Info);

    let colors = fern::colors::ColoredLevelConfig::new()
        .error(fern::colors::Color::Red)
        .warn(fern::colors::Color::Yellow)
        .info(fern::colors::Color::Green)
        .debug(fern::colors::Color::Cyan)
        .trace(fern::colors::Color::White);

    let stdout_dispatch = fern::Dispatch::new()
        .level(stdout_level)
        .format(move |out, message, record| {
            out.finish(format_args!(
                "[{} {:<5} {}] {}",
                chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
                colors.color(record.level()),
                record.target(),
                message
            ))
        })
        .chain(std::io::stdout());

    let mut dispatch = fern::Dispatch::new().chain(stdout_dispatch);

    if let Some(path) = log_path {
        if let Some(file_dispatch) = make_file_dispatch(path) {
            dispatch = dispatch.chain(file_dispatch);
        }
    }

    if let Err(e) = dispatch.apply() {
        eprintln!("failed to initialise logging: {e}");
    }
}

/// Initialise COM (MTA) on the calling thread. Call once before using any UIA functions.
pub fn init_com() {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

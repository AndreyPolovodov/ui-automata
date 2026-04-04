/// UI Live Inspector — tracks the mouse and prints the UIA ancestor tree for
/// the element under the cursor, with an overlay rectangle on screen.
///
/// Press Ctrl-C to exit.

#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
fn main() {
    use std::io::Write;

    automata_windows::init_logging(None);
    automata_windows::init_com();

    let hook = automata_windows::MouseHook::start().expect("failed to install mouse hook");
    let probe = automata_windows::UiaProbe::new().expect("failed to initialise UIA");
    let overlay = automata_windows::Overlay::new().expect("failed to create overlay");

    println!("Live Inspector running. Move mouse over any window. Press Ctrl-C to exit.\n");

    for (x, y) in hook.receiver().iter() {
        if let Some((ancestors, info)) = probe.at_with_ancestors(x, y) {
            print!("\x1b[2J\x1b[H");

            for (i, anc) in ancestors.iter().enumerate() {
                let indent = "   ".repeat(i);
                let connector = if i == 0 { "" } else { "└─ " };
                println!(
                    "{indent}{connector}[{role}] \"{name}\" class={class} id={id}",
                    role = anc.role,
                    name = anc.name,
                    class = anc.class,
                    id = anc.automation_id,
                );
            }

            let depth = ancestors.len();
            let indent = "   ".repeat(depth);
            println!(
                "{indent}└─ [{role}] \"{name}\" class={class} id={id} value={value} \
                 enabled={enabled} rect=({bx},{by},{bw},{bh})",
                role = info.role,
                name = info.name,
                class = info.class,
                id = info.automation_id,
                value = info.value,
                enabled = info.enabled,
                bx = info.x,
                by = info.y,
                bw = info.w,
                bh = info.h,
            );

            let _ = std::io::stdout().flush();
            overlay.show_at(info.x, info.y, info.w, info.h);
        }
    }
}

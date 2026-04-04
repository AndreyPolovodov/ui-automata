/// UI X-Ray: render a window's UIA element tree as an HTML overlay.
///
/// Each element becomes an absolutely-positioned div with a black border,
/// its name centred inside, font sized to fit.
///
/// Usage:  ui-x-ray <window title>
/// Output: <window_title>.html

#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
fn main() {
    let title: String = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        eprintln!("Usage: x-ray <window title>");
        std::process::exit(1);
    }

    automata_windows::init_com();

    let tree = match automata_windows::build_element_tree(
        None,
        Some(&title),
        None,
        None,
        None,
        usize::MAX,
        None,
    ) {
        Ok(t) => t,
        Err(_) => {
            eprintln!("Window not found: {title:?}");
            std::process::exit(1);
        }
    };

    let offset_x = tree.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let offset_y = tree.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let canvas_w = tree.get("width").and_then(|v| v.as_i64()).unwrap_or(0);
    let canvas_h = tree.get("height").and_then(|v| v.as_i64()).unwrap_or(0);

    let mut body = String::new();
    render_node(&tree, offset_x, offset_y, &mut body);

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ background: #f5f5f5; }}
  .canvas {{ position: relative; width: {canvas_w}px; height: {canvas_h}px; background: white; }}
  .el {{
    position: absolute;
    border: 1px solid black;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    text-align: center;
    white-space: nowrap;
    font-family: sans-serif;
    line-height: 1;
    transition: border-color 0.1s, color 0.1s;
    background-color: rgba(255, 255, 255, 0.5);
  }}
  .el:hover {{
    border-color: #e63;
    color: #e63;
  }}
</style>
</head>
<body>
<div class="canvas">
{body}</div>
</body>
</html>
"#,
    );

    let filename = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        + ".html";

    std::fs::write(&filename, &html).expect("Failed to write file");
    eprintln!("Written to {filename}");
}

#[cfg(target_os = "windows")]
fn render_node(node: &serde_json::Value, offset_x: i32, offset_y: i32, out: &mut String) {
    let x = node.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32 - offset_x;
    let y = node.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32 - offset_y;
    let w = node.get("width").and_then(|v| v.as_i64()).unwrap_or(0);
    let h = node.get("height").and_then(|v| v.as_i64()).unwrap_or(0);

    if w > 0 && h > 0 {
        let name_str = node.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let role_str = node.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let fs = if !name_str.is_empty() {
            let by_height = (h as f64 * 0.45).min(13.0);
            let by_width = (w as f64 / name_str.len().max(1) as f64 * 1.5).min(13.0);
            by_height.min(by_width).max(5.0)
        } else {
            0.0
        };
        let name = escape_html(name_str);
        let role = escape_html(role_str);
        out.push_str(&format!(
            "  <div class=\"el\" \
               style=\"left:{x}px;top:{y}px;width:{w}px;height:{h}px;font-size:{fs:.1}px\" \
               title=\"{role}\">{name}</div>\n"
        ));
    }

    if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
        for child in children {
            render_node(child, offset_x, offset_y, out);
        }
    }
}

#[cfg(target_os = "windows")]
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

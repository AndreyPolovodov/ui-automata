use anyhow::Result;
use automata_browser::Browser;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("launch") => {
            let port = parse_port_flag(&args, 2)?;
            let browser = Browser::new(port);
            match browser.ensure_edge().await? {
                (true, actual) => println!("already running on port {actual}"),
                (false, actual) => println!("launched on port {actual}"),
            }
        }

        Some("tabs") => {
            let port = parse_port_flag(&args, 2)?;
            let browser = Browser::new(port);
            let tabs = browser.list_tabs().await?;
            println!("{}", serde_json::to_string_pretty(&tabs)?);
        }

        Some("navigate") => {
            let tab_id = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("missing <tab_id>"))?;
            let url = args
                .get(3)
                .ok_or_else(|| anyhow::anyhow!("missing <url>"))?;
            let port = parse_port_flag(&args, 4)?;
            Browser::new(port).navigate(tab_id, url).await?;
            println!("ok");
        }

        Some("eval") => {
            let tab_id = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("missing <tab_id>"))?;
            let expression = args
                .get(3)
                .ok_or_else(|| anyhow::anyhow!("missing <expression>"))?;
            let port = parse_port_flag(&args, 4)?;
            let value = Browser::new(port).eval(tab_id, expression).await?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }

        Some("dom") => {
            let tab_id = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("missing <tab_id>"))?;
            let port = parse_port_flag(&args, 3)?;
            let tree = Browser::new(port).dom_tree(tab_id).await?;
            println!("{}", serde_json::to_string_pretty(&tree)?);
        }

        Some("screenshot") => {
            let tab_id = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("missing <tab_id>"))?;
            let port = parse_port_flag(&args, 3)?;
            let data = Browser::new(port).screenshot(tab_id).await?;
            println!("{data}");
        }

        Some("activate") => {
            let tab_id = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("missing <tab_id>"))?;
            let port = parse_port_flag(&args, 3)?;
            Browser::new(port).activate_tab(tab_id).await?;
            println!("ok");
        }

        Some("close") => {
            let tab_id = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("missing <tab_id>"))?;
            let port = parse_port_flag(&args, 3)?;
            Browser::new(port).close_tab(tab_id).await?;
            println!("ok");
        }

        Some("open") => {
            let url = args.get(2).map(|s| s.as_str()).unwrap_or("about:blank");
            let port = parse_port_flag(&args, 3)?;
            let tab_id = Browser::new(port).open_tab(url).await?;
            println!("tab_id={tab_id}");
        }

        _ => {
            eprintln!("Usage:");
            eprintln!("  automata-browser launch      [--port 9222]");
            eprintln!("  automata-browser tabs        [--port 9222]");
            eprintln!("  automata-browser open        [<url>]          [--port 9222]");
            eprintln!("  automata-browser navigate    <tab_id> <url>   [--port 9222]");
            eprintln!("  automata-browser eval        <tab_id> <expr>  [--port 9222]");
            eprintln!("  automata-browser dom         <tab_id>         [--port 9222]");
            eprintln!("  automata-browser screenshot  <tab_id>         [--port 9222]");
            eprintln!("  automata-browser activate    <tab_id>         [--port 9222]");
            eprintln!("  automata-browser close       <tab_id>         [--port 9222]");
            std::process::exit(1);
        }
    }

    Ok(())
}

fn parse_port_flag(args: &[String], start_idx: usize) -> Result<u16> {
    let mut i = start_idx;
    while i < args.len() {
        if args[i] == "--port" {
            let val = args
                .get(i + 1)
                .ok_or_else(|| anyhow::anyhow!("--port requires a value"))?;
            return Ok(val
                .parse::<u16>()
                .map_err(|_| anyhow::anyhow!("invalid port: {val}"))?);
        }
        i += 1;
    }
    Ok(automata_browser::DEFAULT_PORT)
}

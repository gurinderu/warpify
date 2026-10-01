//! `warpify` — talks to the warpify zellij plugin of the current session over `zellij pipe`.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader};
use std::process::{Command, ExitCode, Stdio};

use warpify_proto::{Event, Request, State, PIPE_NAME};

const USAGE: &str = "usage: warpify [--plugin <url>] <state|watch>

  state   print the session's tabs and the clients on each
  watch   print the state on every change (heartbeats are silent)

  --plugin <url>  also load the plugin from <url> if the session doesn't run it yet
                  (e.g. file:/path/to/warpify.wasm)";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("warpify: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let mut plugin = None;
    let mut command = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--plugin" => plugin = Some(args.next().ok_or("--plugin needs a url")?),
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ if command.is_none() => command = Some(arg),
            _ => return Err(format!("unexpected argument {arg:?}\n\n{USAGE}")),
        }
    }
    let request = match command.as_deref() {
        Some("state") => Request::State,
        Some("watch") => Request::Watch,
        Some(other) => return Err(format!("unknown command {other:?}\n\n{USAGE}")),
        None => return Err(USAGE.to_owned()),
    };
    let once = request == Request::State;
    stream(&request, plugin.as_deref(), |event| {
        if let Event::State(state) = event {
            print!("{}", render(&state));
        }
        !once
    })
}

/// Sends `request` down the pipe and feeds each reply to `on_event` until it returns `false` or
/// the pipe closes.
fn stream(
    request: &Request,
    plugin: Option<&str>,
    mut on_event: impl FnMut(Event) -> bool,
) -> Result<(), String> {
    let payload = serde_json::to_string(request).map_err(|e| e.to_string())?;
    let mut cmd = Command::new("zellij");
    cmd.args(["pipe", "--name", PIPE_NAME]);
    if let Some(url) = plugin {
        cmd.args(["--plugin", url]);
    }
    cmd.args(["--", &payload]).stdout(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("can't run zellij: {e}"))?;
    let stdout = child.stdout.take().ok_or("zellij gave no stdout")?;
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let event: Event =
            serde_json::from_str(&line).map_err(|e| format!("bad reply {line:?}: {e}"))?;
        if !on_event(event) {
            let _ = child.kill();
            break;
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() || status.code().is_none() {
        Ok(())
    } else {
        Err(format!("zellij pipe exited with {status}"))
    }
}

fn render(state: &State) -> String {
    let mut out = String::new();
    for tab in &state.tabs {
        let clients: Vec<String> = state.clients_on(tab.id).map(|c| c.id.to_string()).collect();
        let _ = writeln!(
            out,
            "{:>3}  {:<24} id={:<4} clients=[{}]",
            tab.position,
            tab.name,
            tab.id,
            clients.join(",")
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use warpify_proto::{Client, Tab};

    #[test]
    fn render_lists_clients_under_their_tab() {
        let state = State {
            tabs: vec![
                Tab {
                    id: 7,
                    position: 0,
                    name: "logs".into(),
                },
                Tab {
                    id: 9,
                    position: 1,
                    name: "edit".into(),
                },
            ],
            clients: vec![
                Client {
                    id: 1,
                    tab: 9,
                    managed: false,
                },
                Client {
                    id: 2,
                    tab: 9,
                    managed: false,
                },
            ],
        };
        let out = render(&state);
        assert!(out.lines().next().unwrap().ends_with("clients=[]"));
        assert!(out.lines().nth(1).unwrap().ends_with("clients=[1,2]"));
    }
}

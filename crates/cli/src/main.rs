//! `warpify` — talks to the warpify zellij plugin of the current session over `zellij pipe`.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader};
use std::process::{ChildStdout, Command, ExitCode, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use warpify_proto::{Event, Request, State, HEARTBEAT_SECS, PIPE_NAME};

const USAGE: &str = "usage: warpify <state|watch>

  state   print the session's tabs and the clients on each
  watch   print the state on every change (heartbeats are silent)

The warpify plugin must already be loaded in the session (zellij config or layout).";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("warpify: {err}");
            ExitCode::FAILURE
        }
    }
}

/// What the command line asks for.
#[derive(Debug, PartialEq)]
enum Cmd {
    Help,
    Send(Request),
}

fn parse_args(args: Vec<String>) -> Result<Cmd, String> {
    let mut command = None;
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Cmd::Help),
            _ if command.is_none() => command = Some(arg),
            _ => return Err(format!("unexpected argument {arg:?}\n\n{USAGE}")),
        }
    }
    match command.as_deref() {
        Some("state") => Ok(Cmd::Send(Request::State)),
        Some("watch") => Ok(Cmd::Send(Request::Watch)),
        Some(other) => Err(format!("unknown command {other:?}\n\n{USAGE}")),
        None => Err(USAGE.to_owned()),
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let request = match parse_args(args)? {
        Cmd::Help => {
            println!("{USAGE}");
            return Ok(());
        }
        Cmd::Send(request) => request,
    };
    let once = request == Request::State;
    stream(&request, |event| {
        if let Event::State(state) = event {
            print!("{}", render(state));
            return !once;
        }
        true
    })
}

/// Sends `request` down the pipe and feeds each reply (heartbeats included) to `on_event` until it
/// returns `false` or the pipe closes. Errors if no State arrives before that, or if nothing at
/// all arrives for three heartbeat periods.
fn stream(request: &Request, mut on_event: impl FnMut(&Event) -> bool) -> Result<(), String> {
    let payload = serde_json::to_string(request).map_err(|e| e.to_string())?;
    let mut child = Command::new("zellij")
        .args(["pipe", "--name", PIPE_NAME, "--", &payload])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("can't run zellij: {e}"))?;
    let stdout = child.stdout.take().ok_or("zellij gave no stdout")?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || read_events(stdout, &tx));

    let silence = Duration::from_secs_f64(3.0 * HEARTBEAT_SECS);
    let mut got_state = false;
    let outcome = loop {
        match rx.recv_timeout(silence) {
            Ok(Ok(event)) => {
                got_state |= matches!(event, Event::State(_));
                if !on_event(&event) {
                    break Ok(());
                }
            }
            Ok(Err(err)) => break Err(err),
            Err(RecvTimeoutError::Timeout) => break Err(not_loaded(silence.as_secs_f64())),
            Err(RecvTimeoutError::Disconnected) => {
                break if got_state {
                    Ok(())
                } else {
                    Err(not_loaded_closed())
                };
            }
        }
    };
    if outcome.is_err() || !got_state {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    outcome?;
    if status.success() || status.code().is_none() {
        Ok(())
    } else {
        Err(format!("zellij pipe exited with {status}"))
    }
}

fn not_loaded(secs: f64) -> String {
    format!("no reply from the warpify plugin for {secs} s — is it loaded in this session?")
}

fn not_loaded_closed() -> String {
    "the pipe closed before the warpify plugin sent a state — is it loaded in this session?"
        .to_owned()
}

/// Reads reply lines from the pipe and forwards them until it closes or the receiver is gone.
fn read_events(stdout: ChildStdout, tx: &mpsc::Sender<Result<Event, String>>) {
    for line in BufReader::new(stdout).lines() {
        let item = match line {
            Ok(line) if line.trim().is_empty() => continue,
            Ok(line) => serde_json::from_str(&line).map_err(|e| format!("bad reply {line:?}: {e}")),
            Err(e) => Err(e.to_string()),
        };
        if tx.send(item).is_err() {
            return;
        }
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

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parse_accepts_state_and_watch() {
        assert_eq!(parse_args(args(&["state"])), Ok(Cmd::Send(Request::State)));
        assert_eq!(parse_args(args(&["watch"])), Ok(Cmd::Send(Request::Watch)));
    }

    #[test]
    fn parse_rejects_plugin_option() {
        assert!(parse_args(args(&["--plugin", "file:x.wasm", "state"])).is_err());
        assert!(parse_args(args(&["state", "--plugin"])).is_err());
    }

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

//! `warpify` — talks to the warpify zellij plugin of the current session over `zellij pipe`.

use std::fmt::Write as _;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use warpify_proto::{Event, Request, State};

/// Talks to the warpify zellij plugin of the current session over `zellij pipe`. The plugin must
/// already be loaded in the session (zellij config or layout).
#[derive(Parser)]
#[command(name = "warpify", about)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// print the session's tabs and the clients on each
    State,
    /// print the state on every change (heartbeats are silent)
    Watch,
}

fn main() -> ExitCode {
    match run(&Cli::parse().command.into()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("warpify: {err}");
            ExitCode::FAILURE
        }
    }
}

impl From<Cmd> for Request {
    fn from(command: Cmd) -> Self {
        match command {
            Cmd::State => Request::State,
            Cmd::Watch => Request::Watch,
        }
    }
}

fn run(request: &Request) -> Result<(), warpify_client::Error> {
    let once = *request == Request::State;
    let mut changes = warpify_client::Changes::default();
    warpify_client::stream(request, |event| {
        if let Event::State(state) = event {
            if changes.is_new(state) {
                print!("{}", render(state));
            }
            return !once;
        }
        true
    })
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
    use clap::CommandFactory;
    use warpify_proto::{Client, Tab};

    #[test]
    fn cli_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parse_accepts_state_and_watch() {
        assert!(matches!(
            Cli::try_parse_from(["warpify", "state"]).unwrap().command,
            Cmd::State
        ));
        assert!(matches!(
            Cli::try_parse_from(["warpify", "watch"]).unwrap().command,
            Cmd::Watch
        ));
    }

    #[test]
    fn parse_rejects_plugin_option() {
        assert!(Cli::try_parse_from(["warpify", "--plugin", "x", "state"]).is_err());
        assert!(Cli::try_parse_from(["warpify", "state", "--plugin"]).is_err());
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

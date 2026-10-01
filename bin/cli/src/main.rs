//! `warpify` — talks to the warpify zellij plugin of a zellij session over `zellij pipe`.

use std::io::{self, BufWriter, Write};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use warpify_proto::{Event, Request, State};

/// Talks to the warpify zellij plugin of a session (the current one, or --session) over
/// `zellij pipe`. The plugin must already be loaded in the session (zellij config or layout).
#[derive(Parser)]
#[command(name = "warpify", about)]
struct Cli {
    /// log diagnostics to stderr: -v debug, -vv trace (`RUST_LOG` overrides)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
    /// send to this zellij session instead of the current one
    #[arg(short, long, value_name = "NAME", global = true)]
    session: Option<String>,
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
    let cli = Cli::parse();
    let level = match cli.verbose {
        0 => "warn",
        1 => "debug",
        _ => "trace",
    };
    warpify_telemetry::init(&format!(
        "warpify={level},warpify_client={level},warpify_session={level}"
    ));
    let target = cli.session.map_or(
        warpify_client::Target::Current,
        warpify_client::Target::Named,
    );
    match run(&cli.command.into(), &target) {
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

fn run(
    request: &Request,
    target: &warpify_client::Target,
) -> Result<(), Box<dyn std::error::Error>> {
    let once = *request == Request::State;
    let mut changes = warpify_client::Changes::default();
    let mut out = BufWriter::new(io::stdout().lock());
    let mut write_err = None;
    warpify_client::stream(request, target, |event| {
        if let Event::State(state) = event {
            if changes.is_new(state) {
                // Flushed per state: `watch` must show each one at once.
                if let Err(err) = render(state, &mut out).and_then(|()| out.flush()) {
                    write_err = Some(err);
                    return false;
                }
            }
            return !once;
        }
        true
    })?;
    match write_err {
        // The reader went away (e.g. `warpify watch | head`): a clean exit.
        Some(err) if err.kind() != io::ErrorKind::BrokenPipe => Err(err.into()),
        _ => Ok(()),
    }
}

fn render(state: &State, out: &mut impl Write) -> io::Result<()> {
    for tab in &state.tabs {
        let clients: Vec<String> = state.clients_on(tab.id).map(|c| c.id.to_string()).collect();
        writeln!(
            out,
            "{:>3}  {:<24} id={:<4} clients=[{}]",
            tab.position,
            tab.name,
            tab.id,
            clients.join(",")
        )?;
    }
    Ok(())
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
    fn verbose_counts() {
        let cli = Cli::try_parse_from(["warpify", "-vv", "state"]).unwrap();
        assert_eq!(cli.verbose, 2);
        let cli = Cli::try_parse_from(["warpify", "watch", "-v"]).unwrap();
        assert_eq!(cli.verbose, 1);
    }

    #[test]
    fn session_option_is_global() {
        let cli = Cli::try_parse_from(["warpify", "state", "-s", "w"]).unwrap();
        assert_eq!(cli.session.as_deref(), Some("w"));
        let cli = Cli::try_parse_from(["warpify", "--session", "w", "watch"]).unwrap();
        assert_eq!(cli.session.as_deref(), Some("w"));
        assert!(Cli::try_parse_from(["warpify", "state"])
            .unwrap()
            .session
            .is_none());
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
        let mut buf = Vec::new();
        render(&state, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.lines().next().unwrap().ends_with("clients=[]"));
        assert!(out.lines().nth(1).unwrap().ends_with("clients=[1,2]"));
    }
}

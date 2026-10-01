//! `warpify` — talks to the warpify zellij plugin of a zellij session over `zellij pipe`.

use std::io::{self, BufWriter, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode, Stdio};

use clap::{Args, Parser, Subcommand};
use warpify_client::SessionTarget;
use warpify_proto::{ClientId, Event, Request, State, TabId, Target};

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
    /// move a client to a tab and mark it managed
    Bind {
        /// the client to move (see `state`)
        #[arg(long, value_name = "ID")]
        client: ClientId,
        #[command(flatten)]
        target: TargetArgs,
        /// send the client back to its tab whenever it wanders off
        #[arg(long)]
        pin: bool,
    },
    /// start a zellij client in a session (default `warpify`, created if missing) and bind it
    #[command(mut_group("target", |g| g.required(false)))]
    Attach {
        #[command(flatten)]
        target: TargetArgs,
        /// send the client back to its tab whenever it wanders off
        #[arg(long)]
        pin: bool,
    },
    /// bind the next client that connects (spawned by `attach`)
    #[command(name = "__bind-new", hide = true, mut_group("target", |g| g.required(false)))]
    BindNew {
        /// comma-separated ids of the clients already connected
        #[arg(long, value_name = "IDS", default_value = "")]
        known: String,
        #[command(flatten)]
        target: TargetArgs,
        #[arg(long)]
        pin: bool,
    },
}

/// Where a bind puts the client; exactly one.
#[derive(Args)]
#[group(id = "target", required = true, multiple = false)]
struct TargetArgs {
    /// an existing tab by its id
    #[arg(long, value_name = "ID")]
    tab_id: Option<TabId>,
    /// the tab with this name, created if missing
    #[arg(long, value_name = "NAME")]
    name: Option<String>,
    /// a fresh tab, optionally named
    #[arg(long, value_name = "NAME", num_args = 0..=1, require_equals = false)]
    new: Option<Vec<String>>,
}

impl TargetArgs {
    /// The chosen target; a fresh unnamed tab when none was given (`attach`).
    fn target(&self) -> Target {
        match (self.tab_id, &self.name, &self.new) {
            (Some(id), _, _) => Target::Id(id),
            (_, Some(name), _) => Target::Name(name.clone()),
            (_, _, Some(name)) => Target::New(name.first().cloned()),
            _ => Target::New(None),
        }
    }
}

/// The flags that spell `target` for a re-run of this binary.
fn target_flags(target: &Target) -> Vec<String> {
    match target {
        Target::Id(id) => vec!["--tab-id".into(), id.to_string()],
        Target::Name(name) => vec!["--name".into(), name.clone()],
        Target::New(None) => vec!["--new".into()],
        Target::New(Some(name)) => vec![format!("--new={name}")],
    }
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
    let result = match cli.command {
        Cmd::State => run(&Request::State, &session_target(cli.session)),
        Cmd::Watch => run(&Request::Watch, &session_target(cli.session)),
        Cmd::Bind {
            client,
            target,
            pin,
        } => bind(&session_target(cli.session), client, &target.target(), pin),
        Cmd::Attach { target, pin } => attach(cli.session.as_deref(), &target.target(), pin),
        Cmd::BindNew { known, target, pin } => {
            bind_new(cli.session.as_deref(), &known, &target.target(), pin)
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("warpify: {err}");
            ExitCode::FAILURE
        }
    }
}

fn session_target(named: Option<String>) -> SessionTarget {
    named.map_or(SessionTarget::Current, SessionTarget::Named)
}

type Outcome = Result<(), Box<dyn std::error::Error>>;

fn bind(session: &SessionTarget, client: ClientId, target: &Target, pin: bool) -> Outcome {
    let tab = warpify_client::bind_and_confirm(session, client, target, pin)?;
    writeln!(
        io::stdout(),
        "client {client} → tab \"{}\" (id {})",
        tab.name,
        tab.id
    )?;
    Ok(())
}

/// Snapshots the session's clients, starts the detached `__bind-new` helper, then becomes
/// `zellij attach --create` (graph @nick/warpify, node #16).
fn attach(session: Option<&str>, target: &Target, pin: bool) -> Outcome {
    warpify_client::check_outside_zellij(std::env::var("ZELLIJ_SESSION_NAME").ok().as_deref())?;
    let session = warpify_client::attach_session(session);
    let known: Vec<String> =
        match warpify_client::fetch_state(&SessionTarget::Named(session.into())) {
            Ok(state) => state.clients.iter().map(|c| c.id.to_string()).collect(),
            Err(err) => {
                tracing::debug!(%err, "no state before attach; assuming no clients");
                Vec::new()
            }
        };
    let mut helper = Command::new(std::env::current_exe()?);
    helper
        .args(["__bind-new", "-s", session])
        .arg(format!("--known={}", known.join(",")))
        .args(target_flags(target))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if pin {
        helper.arg("--pin");
    }
    helper.spawn()?;
    let err = Command::new("zellij")
        .args(["attach", "--create", session])
        .exec();
    Err(format!("can't run zellij: {err}").into())
}

fn bind_new(session: Option<&str>, known: &str, target: &Target, pin: bool) -> Outcome {
    let session = SessionTarget::Named(warpify_client::attach_session(session).into());
    let known = warpify_client::parse_known(known)?;
    let client = warpify_client::wait_for_new_client(&session, &known)?;
    tracing::debug!(client, "binding the new client");
    warpify_client::send(
        &Request::Bind {
            client,
            target: target.clone(),
            pin,
        },
        &session,
    )?;
    Ok(())
}

fn run(request: &Request, target: &SessionTarget) -> Result<(), Box<dyn std::error::Error>> {
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
    fn bind_needs_exactly_one_target() {
        let parse = |args: &[&str]| {
            Cli::try_parse_from(["warpify", "bind", "--client", "2"].iter().chain(args))
        };
        assert!(parse(&[]).is_err());
        assert!(parse(&["--tab-id", "1", "--name", "x"]).is_err());
        assert!(Cli::try_parse_from(["warpify", "bind", "--tab-id", "1"]).is_err());
        let cli = parse(&["--new", "--pin"]).unwrap();
        let Cmd::Bind { target, pin, .. } = cli.command else {
            panic!("not bind")
        };
        assert!(pin);
        assert_eq!(target.target(), Target::New(None));
        let Cmd::Bind { target, .. } = parse(&["--new", "logs"]).unwrap().command else {
            panic!("not bind")
        };
        assert_eq!(target.target(), Target::New(Some("logs".into())));
        let Cmd::Bind { target, .. } = parse(&["--tab-id", "4"]).unwrap().command else {
            panic!("not bind")
        };
        assert_eq!(target.target(), Target::Id(4));
    }

    #[test]
    fn attach_target_is_optional_and_defaults_to_new() {
        let Cmd::Attach { target, pin } =
            Cli::try_parse_from(["warpify", "attach"]).unwrap().command
        else {
            panic!("not attach")
        };
        assert!(!pin);
        assert_eq!(target.target(), Target::New(None));
        assert!(Cli::try_parse_from(["warpify", "attach", "--name", "a", "--new"]).is_err());
    }

    #[test]
    fn helper_flags_round_trip() {
        for target in [
            Target::Id(3),
            Target::Name("a b".into()),
            Target::New(None),
            Target::New(Some("n".into())),
        ] {
            let mut argv = vec!["warpify".to_owned(), "__bind-new".to_owned()];
            argv.push("--known=1,2".into());
            argv.extend(target_flags(&target));
            let Cmd::BindNew {
                known,
                target: parsed,
                ..
            } = Cli::try_parse_from(argv).unwrap().command
            else {
                panic!("not bind-new")
            };
            assert_eq!(known, "1,2");
            assert_eq!(parsed.target(), target);
        }
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

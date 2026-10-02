//! `warpify` — talks to the warpify zellij plugin of a zellij session over `zellij pipe`.

use std::io::{self, BufWriter, Write};
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use warpify_client::SessionTarget;
use warpify_proto::{ClientId, Event, Request, State, TabId, Target};

mod install;

/// What `install`/`uninstall` set up; more integrations fit here later.
#[derive(Clone, Copy, ValueEnum)]
enum Integration {
    Zellij,
}

/// What the plugin does for a client that connects (`install zellij --on-connect`).
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OnConnect {
    /// leave the client where zellij put it
    None,
    /// a client alone in the session when it connects stays on its tab, any other gets a new tab of its own
    NewTab,
}

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
    /// install the plugin for an integration (zellij: download, grant permissions, load it)
    Install {
        integration: Integration,
        /// use this local plugin build instead of downloading the release
        #[arg(long, value_name = "PATH")]
        wasm: Option<std::path::PathBuf>,
        /// what the plugin does for a client that connects; written into the plugin's
        /// `load_plugins` entry, so a re-install without it resets to `none`
        #[arg(long, value_enum, value_name = "ACTION")]
        on_connect: Option<OnConnect>,
        /// keep a client bound on connect on its tab (needs --on-connect new-tab)
        #[arg(long, requires = "on_connect")]
        pin: bool,
        /// show the host and the session's tabs in the terminal title (what Warp shows as the
        /// tab title); the plugin renames the client's focused pane
        #[arg(long)]
        title: bool,
        /// the title's first part; default: an emoji for the OS and the short host name
        #[arg(long, value_name = "TEXT", requires = "title")]
        title_prefix: Option<String>,
        /// print what would be done, change nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// undo `install`
    Uninstall {
        integration: Integration,
        /// print what would be done, change nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// grant the plugin at an exact path its permissions, nothing else (for nix/home-manager)
    #[command(name = "__grant-permissions", hide = true)]
    GrantPermissions {
        integration: Integration,
        /// absolute path zellij loads the plugin from
        #[arg(long, value_name = "ABS PATH")]
        wasm_path: std::path::PathBuf,
    },
    /// move a client to a tab and bind it there
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
    /// The chosen target.
    fn target(&self) -> Target {
        match (self.tab_id, &self.name, &self.new) {
            (Some(id), _, _) => Target::Id(id),
            (_, Some(name), _) => Target::Name(name.clone()),
            (_, _, Some(name)) => Target::New(name.first().cloned()),
            _ => Target::New(None),
        }
    }
}

impl Cli {
    /// Parse `args`, then reject what a single clap rule can't: `--pin` with `--on-connect none`.
    fn parse_checked<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let cli = Self::try_parse_from(args)?;
        if let Cmd::Install {
            on_connect: Some(OnConnect::None),
            pin: true,
            ..
        } = cli.command
        {
            return Err(Self::command().error(
                ErrorKind::ArgumentConflict,
                "--pin needs `--on-connect new-tab`: with `--on-connect none` nothing is bound",
            ));
        }
        Ok(cli)
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse_checked(std::env::args_os()).unwrap_or_else(|err| err.exit());
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
        Cmd::Install {
            integration,
            wasm,
            on_connect,
            pin,
            title,
            title_prefix,
            dry_run,
        } => install::install(
            integration,
            wasm,
            warpify_install::PluginOptions {
                new_tab_on_connect: on_connect == Some(OnConnect::NewTab),
                pin,
                title: title.then(|| title_prefix.unwrap_or_else(install::this_machine_prefix)),
            },
            dry_run,
        ),
        Cmd::Uninstall {
            integration,
            dry_run,
        } => install::uninstall(integration, dry_run),
        Cmd::GrantPermissions {
            integration,
            wasm_path,
        } => install::grant_permissions(integration, &wasm_path),
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
        "client {client} is on tab \"{}\" (id {})",
        tab.name,
        tab.id
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
    fn install_parses_integration_and_flags() {
        let Cmd::Install {
            wasm,
            dry_run,
            on_connect,
            pin,
            ..
        } = Cli::try_parse_from(["warpify", "install", "zellij", "--wasm", "/w", "--dry-run"])
            .unwrap()
            .command
        else {
            panic!("not install")
        };
        assert_eq!(wasm, Some("/w".into()));
        assert!(dry_run);
        assert!(on_connect.is_none() && !pin);
        assert!(Cli::try_parse_from(["warpify", "uninstall", "zellij"]).is_ok());
        assert!(Cli::try_parse_from(["warpify", "uninstall", "zellij", "--wasm", "x"]).is_err());
        assert!(Cli::try_parse_from(["warpify", "install", "tmux"]).is_err());
    }

    #[test]
    fn install_takes_title_and_its_prefix() {
        let parse = |args: &[&str]| {
            Cli::try_parse_from(["warpify", "install", "zellij"].iter().chain(args))
        };
        let Cmd::Install {
            title,
            title_prefix,
            ..
        } = parse(&["--title", "--title-prefix", "🟠 x"])
            .unwrap()
            .command
        else {
            panic!("not install")
        };
        assert!(title && title_prefix.as_deref() == Some("🟠 x"));
        assert!(parse(&["--title"]).is_ok());
        assert!(parse(&["--title-prefix", "x"]).is_err());
    }

    #[test]
    fn install_takes_on_connect_and_pin() {
        let parse = |args: &[&str]| {
            Cli::try_parse_from(["warpify", "install", "zellij"].iter().chain(args))
        };
        let Cmd::Install {
            on_connect, pin, ..
        } = parse(&["--on-connect", "new-tab", "--pin"])
            .unwrap()
            .command
        else {
            panic!("not install")
        };
        assert!(on_connect == Some(OnConnect::NewTab) && pin);
        let Cmd::Install { on_connect, .. } = parse(&["--on-connect", "none"]).unwrap().command
        else {
            panic!("not install")
        };
        assert!(on_connect == Some(OnConnect::None));
        assert!(parse(&["--on-connect", "new_tab"]).is_err());
        assert!(parse(&["--pin"]).is_err());
        assert!(Cli::parse_checked([
            "warpify",
            "install",
            "zellij",
            "--on-connect",
            "new-tab",
            "--pin"
        ])
        .is_ok());
        let err = Cli::parse_checked([
            "warpify",
            "install",
            "zellij",
            "--on-connect",
            "none",
            "--pin",
        ])
        .err()
        .expect("none + pin is rejected");
        assert!(err.to_string().contains("--pin needs"), "{err}");
        assert!(Cli::try_parse_from(["warpify", "uninstall", "zellij", "--pin"]).is_err());
    }

    #[test]
    fn grant_permissions_is_hidden_and_takes_a_path() {
        use clap::CommandFactory;
        let argv = [
            "warpify",
            "__grant-permissions",
            "zellij",
            "--wasm-path",
            "/w",
        ];
        assert!(Cli::try_parse_from(argv).is_ok());
        assert!(Cli::command()
            .get_subcommands()
            .any(|c| c.get_name() == "__grant-permissions" && c.is_hide_set()));
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
            clients: vec![Client { id: 1, tab: 9 }, Client { id: 2, tab: 9 }],
        };
        let mut buf = Vec::new();
        render(&state, &mut buf).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.lines().next().unwrap().ends_with("clients=[]"));
        assert!(out.lines().nth(1).unwrap().ends_with("clients=[1,2]"));
    }
}

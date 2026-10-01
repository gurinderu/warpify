//! The CLI side of the warpify pipe: runs `zellij pipe`, reads the plugin's NDJSON replies and
//! enforces the liveness timeout.

use std::fmt;
use std::io::{BufRead, BufReader};
use std::process::{ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use warpify_proto::{ClientId, Event, Request, State, HEARTBEAT_SECS, PIPE_NAME};

mod bind;

pub use bind::{
    attach_session, bind_and_confirm, check_outside_zellij, confirmed, parse_known, pick_new,
    wait_for_new_client, DEFAULT_SESSION,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// `zellij` couldn't be started or waited on.
    Zellij(String),
    /// A reply line wasn't a valid `Event`, or the pipe couldn't be read.
    Reply(String),
    /// No reply for three heartbeat periods.
    Silent { secs: f64 },
    /// The pipe closed before any state arrived.
    Closed,
    /// `zellij pipe` exited with a failure.
    Exit(String),
    /// No session was named and the process isn't running inside one.
    NoSession,
    /// `attach` was run from a pane of a zellij session.
    InsideSession(String),
    /// A `--known` list wasn't comma-separated client ids.
    BadKnown(String),
    /// The bind went out but the client never reached its target.
    NotMoved(ClientId),
    /// No new client connected in time.
    NoNewClient,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Zellij(msg) | Error::Reply(msg) => f.write_str(msg),
            Error::Silent { secs } => write!(
                f,
                "no reply from the warpify plugin for {secs} s — is the warpify plugin loaded in this session, and are its permissions granted?"
            ),
            Error::Closed => f.write_str(
                "the pipe closed before the warpify plugin sent a state — is the warpify plugin loaded in this session, and are its permissions granted?",
            ),
            Error::Exit(status) => write!(f, "zellij pipe exited with {status}"),
            Error::NoSession => f.write_str(
                "not inside a zellij session — run from a pane of the session or pass --session <name>",
            ),
            Error::InsideSession(name) => write!(
                f,
                "already inside zellij session \"{name}\" — use bind instead"
            ),
            Error::BadKnown(item) => write!(f, "bad client id {item:?} in --known"),
            Error::NotMoved(client) => write!(
                f,
                "bind sent but client {client} didn't move — is it connected?"
            ),
            Error::NoNewClient => f.write_str("no new client connected to the session in time"),
        }
    }
}

impl std::error::Error for Error {}

/// Which zellij session to talk to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionTarget {
    /// The session this process runs in (`ZELLIJ_SESSION_NAME`).
    Current,
    /// The session with this name.
    Named(String),
}

/// Decides the session to address: the named one, else the current one if `env_session` (the value
/// of `ZELLIJ_SESSION_NAME`) is set and non-empty. `Ok(None)` means "zellij picks the current one".
fn resolve_session<'a>(
    target: &'a SessionTarget,
    env_session: Option<&str>,
) -> Result<Option<&'a str>, Error> {
    match target {
        SessionTarget::Named(name) => Ok(Some(name)),
        SessionTarget::Current if env_session.is_some_and(|s| !s.is_empty()) => Ok(None),
        SessionTarget::Current => Err(Error::NoSession),
    }
}

/// Removes ANSI escape sequences (CSI `ESC [ … final-byte`) from `text`.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
        } else if chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    out
}

/// Passes a state through only when it differs from the last one passed: several plugin
/// instances may answer the same watch, and their identical states must print once.
#[derive(Debug, Default)]
pub struct Changes {
    last: Option<State>,
}

impl Changes {
    /// Whether `state` is new, remembering it if so.
    pub fn is_new(&mut self, state: &State) -> bool {
        if self.last.as_ref() == Some(state) {
            return false;
        }
        self.last = Some(state.clone());
        true
    }
}

/// Sends `request` to the `target` session's pipe and feeds each reply (heartbeats included) to `on_event` until it
/// returns `false` or the pipe closes.
///
/// # Errors
/// If there is no session to address, if no State arrives before the pipe closes, if nothing at all arrives for three heartbeat
/// periods, if a reply can't be parsed, or if `zellij` fails to run or exits with a failure.
pub fn stream(
    request: &Request,
    target: &SessionTarget,
    mut on_event: impl FnMut(&Event) -> bool,
) -> Result<(), Error> {
    let env_session = std::env::var("ZELLIJ_SESSION_NAME").ok();
    let session = resolve_session(target, env_session.as_deref())?;
    let payload = serde_json::to_string(request).map_err(|e| Error::Zellij(e.to_string()))?;
    tracing::debug!(payload, "spawning zellij pipe");
    let mut command = Command::new("zellij");
    if let Some(name) = session {
        command.args(["--session", name]);
    }
    let mut child = command
        .args(["pipe", "--name", PIPE_NAME, "--", &payload])
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Zellij(format!("can't run zellij: {e}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Zellij("zellij gave no stdout".to_owned()))?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || read_events(stdout, &tx));

    let silence = Duration::from_secs_f64(3.0 * HEARTBEAT_SECS);
    let mut got_state = false;
    let outcome = loop {
        match rx.recv_timeout(silence) {
            Ok(Ok(event)) => {
                tracing::debug!(?event, "event received; heartbeat timer reset");
                got_state |= matches!(event, Event::State(_));
                if !on_event(&event) {
                    break Ok(());
                }
            }
            Ok(Err(err)) => break Err(err),
            Err(RecvTimeoutError::Timeout) => {
                tracing::debug!(secs = silence.as_secs_f64(), "heartbeat timeout");
                break Err(Error::Silent {
                    secs: silence.as_secs_f64(),
                });
            }
            Err(RecvTimeoutError::Disconnected) => {
                break if got_state {
                    Ok(())
                } else {
                    Err(Error::Closed)
                };
            }
        }
    };
    if outcome.is_err() || !got_state {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|e| Error::Zellij(e.to_string()))?;
    outcome?;
    if status.success() || status.code().is_none() {
        Ok(())
    } else {
        Err(Error::Exit(status.to_string()))
    }
}

/// Sends `request` to the `target` session's pipe without waiting for a reply: runs `zellij pipe`
/// to completion (a `Bind` gets no answer).
///
/// # Errors
/// If there is no session to address, if `zellij` fails to run, or if it exits with a failure.
pub fn send(request: &Request, target: &SessionTarget) -> Result<(), Error> {
    let env_session = std::env::var("ZELLIJ_SESSION_NAME").ok();
    let session = resolve_session(target, env_session.as_deref())?;
    let payload = serde_json::to_string(request).map_err(|e| Error::Zellij(e.to_string()))?;
    tracing::debug!(payload, "sending to zellij pipe");
    let mut command = Command::new("zellij");
    if let Some(name) = session {
        command.args(["--session", name]);
    }
    let status = command
        .args(["pipe", "--name", PIPE_NAME, "--", &payload])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .map_err(|e| Error::Zellij(format!("can't run zellij: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Exit(status.to_string()))
    }
}

/// Fetches one `State` from the `target` session.
///
/// # Errors
/// As [`stream`].
pub fn fetch_state(target: &SessionTarget) -> Result<State, Error> {
    let mut state = None;
    stream(&Request::State, target, |event| {
        if let Event::State(s) = event {
            state = Some(s.clone());
            return false;
        }
        true
    })?;
    state.ok_or(Error::Closed)
}

/// Parses one reply line; blank lines carry nothing and give `None`.
fn parse_line(line: &str) -> Option<Result<Event, Error>> {
    if line.trim().is_empty() {
        return None;
    }
    Some(
        serde_json::from_str(line)
            .map_err(|e| Error::Reply(format!("bad reply {:?}: {e}", strip_ansi(line)))),
    )
}

/// Reads reply lines from the pipe and forwards them until it closes or the receiver is gone.
fn read_events(stdout: ChildStdout, tx: &mpsc::Sender<Result<Event, Error>>) {
    for line in BufReader::new(stdout).lines() {
        tracing::trace!(?line, "raw line");
        let item = match line {
            Ok(line) => match parse_line(&line) {
                Some(item) => item,
                None => continue,
            },
            Err(e) => Err(Error::Reply(e.to_string())),
        };
        if tx.send(item).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_good_line() {
        let event = parse_line(r#"{"event":"heartbeat"}"#).unwrap().unwrap();
        assert_eq!(event, Event::Heartbeat);
    }

    #[test]
    fn skips_blank_lines() {
        assert!(parse_line("").is_none());
        assert!(parse_line("  \t").is_none());
    }

    #[test]
    fn bad_json_is_an_error() {
        let err = parse_line("not json").unwrap().unwrap_err();
        assert!(matches!(err, Error::Reply(_)));
        assert!(err.to_string().starts_with("bad reply \"not json\""));
    }

    #[test]
    fn bad_reply_strips_ansi() {
        let err = parse_line("\u{1b}[32;1mwtest\u{1b}[m [Created 3h ago]")
            .unwrap()
            .unwrap_err();
        assert!(err
            .to_string()
            .starts_with("bad reply \"wtest [Created 3h ago]\": "));
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn session_resolution() {
        let named = SessionTarget::Named("w".into());
        assert_eq!(
            resolve_session(&SessionTarget::Current, Some("s")),
            Ok(None)
        );
        assert_eq!(
            resolve_session(&SessionTarget::Current, None),
            Err(Error::NoSession)
        );
        assert_eq!(
            resolve_session(&SessionTarget::Current, Some("")),
            Err(Error::NoSession)
        );
        assert_eq!(resolve_session(&named, None), Ok(Some("w")));
        assert_eq!(resolve_session(&named, Some("s")), Ok(Some("w")));
        assert!(Error::NoSession
            .to_string()
            .starts_with("not inside a zellij session"));
    }

    #[test]
    fn changes_pass_only_differing_states() {
        let state = |name: &str| State {
            tabs: vec![warpify_proto::Tab {
                id: 0,
                position: 0,
                name: name.into(),
            }],
            clients: vec![],
        };
        let mut changes = Changes::default();
        assert!(changes.is_new(&state("a")));
        assert!(!changes.is_new(&state("a")));
        assert!(changes.is_new(&state("b")));
        assert!(changes.is_new(&state("a")));
    }

    #[test]
    fn no_reply_errors_ask_whether_the_plugin_is_loaded() {
        assert!(Error::Silent { secs: 15.0 }
            .to_string()
            .ends_with("are its permissions granted?"));
        assert!(Error::Closed
            .to_string()
            .ends_with("are its permissions granted?"));
    }
}

//! The CLI side of the warpify pipe: runs `zellij pipe`, reads the plugin's NDJSON replies and
//! enforces the liveness timeout.

use std::fmt;
use std::io::{BufRead, BufReader};
use std::process::{ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use warpify_proto::{Event, Request, State, HEARTBEAT_SECS, PIPE_NAME};

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
        }
    }
}

impl std::error::Error for Error {}

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

/// Sends `request` down the pipe and feeds each reply (heartbeats included) to `on_event` until it
/// returns `false` or the pipe closes.
///
/// # Errors
/// If no State arrives before the pipe closes, if nothing at all arrives for three heartbeat
/// periods, if a reply can't be parsed, or if `zellij` fails to run or exits with a failure.
pub fn stream(request: &Request, mut on_event: impl FnMut(&Event) -> bool) -> Result<(), Error> {
    let payload = serde_json::to_string(request).map_err(|e| Error::Zellij(e.to_string()))?;
    let mut child = Command::new("zellij")
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
                got_state |= matches!(event, Event::State(_));
                if !on_event(&event) {
                    break Ok(());
                }
            }
            Ok(Err(err)) => break Err(err),
            Err(RecvTimeoutError::Timeout) => {
                break Err(Error::Silent {
                    secs: silence.as_secs_f64(),
                })
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

/// Parses one reply line; blank lines carry nothing and give `None`.
fn parse_line(line: &str) -> Option<Result<Event, Error>> {
    if line.trim().is_empty() {
        return None;
    }
    Some(serde_json::from_str(line).map_err(|e| Error::Reply(format!("bad reply {line:?}: {e}"))))
}

/// Reads reply lines from the pipe and forwards them until it closes or the receiver is gone.
fn read_events(stdout: ChildStdout, tx: &mpsc::Sender<Result<Event, Error>>) {
    for line in BufReader::new(stdout).lines() {
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

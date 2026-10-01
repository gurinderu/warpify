//! Wire types shared by the zellij plugin and the `warpify` CLI.
//!
//! The CLI talks to the plugin with `zellij pipe --name warpify -- <Request as JSON>`;
//! the plugin answers on the pipe's output with newline-delimited JSON.

use serde::{Deserialize, Serialize};

/// Pipe name the plugin listens on.
pub const PIPE_NAME: &str = "warpify";

/// Seconds between heartbeats on a `watch` stream.
pub const HEARTBEAT_SECS: f64 = 5.0;

pub type ClientId = u16;
pub type TabId = usize;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    /// Reply once with the current `State`.
    State,
    /// Keep the pipe open and send a `Event::State` on every change plus periodic heartbeats.
    Watch,
    /// Put `client` on `target` and mark it as managed: it's detached when its tab closes, and
    /// with `pin` it's sent back to its tab whenever it wanders off.
    Bind {
        client: ClientId,
        target: Target,
        pin: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Target {
    /// An existing tab by its stable id.
    Id(TabId),
    /// The tab with this name, created if missing.
    Name(String),
    /// A fresh tab, optionally named.
    New(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    State(State),
    Heartbeat,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub tabs: Vec<Tab>,
    pub clients: Vec<Client>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub id: TabId,
    pub position: usize,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Client {
    pub id: ClientId,
    pub tab: TabId,
    /// Bound by `warpify attach`. Filled in by the CLI from its registry; the plugin always
    /// reports `false`.
    #[serde(default)]
    pub managed: bool,
}

impl State {
    #[must_use]
    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    #[must_use]
    pub fn client(&self, id: ClientId) -> Option<&Client> {
        self.clients.iter().find(|c| c.id == id)
    }

    pub fn clients_on(&self, tab: TabId) -> impl Iterator<Item = &Client> {
        self.clients.iter().filter(move |c| c.tab == tab)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_format() {
        let bind = Request::Bind {
            client: 3,
            target: Target::Name("logs".into()),
            pin: true,
        };
        let json = serde_json::to_string(&bind).unwrap();
        assert_eq!(
            json,
            r#"{"cmd":"bind","client":3,"target":{"kind":"name","value":"logs"},"pin":true}"#
        );
        assert_eq!(serde_json::from_str::<Request>(&json).unwrap(), bind);
        assert_eq!(
            serde_json::from_str::<Request>(r#"{"cmd":"watch"}"#).unwrap(),
            Request::Watch
        );
    }

    #[test]
    fn new_tab_target_without_name() {
        let json = serde_json::to_string(&Target::New(None)).unwrap();
        assert_eq!(json, r#"{"kind":"new","value":null}"#);
    }
}

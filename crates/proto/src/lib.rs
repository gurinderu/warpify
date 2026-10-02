//! Wire types shared by the zellij plugin and the `warpify` CLI.
//!
//! The CLI talks to the plugin with `zellij pipe --name warpify -- <Request as JSON>`;
//! the plugin answers on the pipe's output with newline-delimited JSON.

use serde::{Deserialize, Serialize};

/// Pipe name the plugin listens on.
pub const PIPE_NAME: &str = "warpify";

/// The zellij permissions the plugin requests, by the names zellij uses (`PermissionType`
/// variants in zellij-utils `plugin_permission.proto`). The plugin builds its
/// `request_permission` list from this and the installer pre-grants exactly these in zellij's
/// permission cache (graph @nick/warpify, node #17).
pub const PERMISSIONS: &[&str] = &[
    "ReadApplicationState",
    "ChangeApplicationState",
    "ReadCliPipes",
    // To tell the other instances a client has left (graph @nick/warpify, node #9).
    "MessageAndLaunchOtherPlugins",
];

/// Plugin configuration keys and values: the children of the plugin's `load_plugins` entry,
/// which zellij hands to `load` as a string map (zellij-utils `kdl_layout_parser.rs`
/// `parse_plugin_user_configuration`). The plugin parses them, the installer and the
/// home-manager module write them (graph @nick/warpify, node #16).
pub const CONFIG_ON_CONNECT: &str = "on_connect";
pub const CONFIG_PIN: &str = "pin";
/// Show the remote description and the tab list in the terminal title (graph @nick/warpify,
/// node #23). Not `title`: zellij strips that key from a plugin's configuration
/// (zellij-utils 0.45.1, `input/layout.rs` `PluginUserConfiguration::new`).
pub const CONFIG_TITLE: &str = "terminal_title";
/// Free text that opens the title, e.g. an OS emoji and the host name.
pub const CONFIG_TITLE_PREFIX: &str = "title_prefix";
pub const ON_CONNECT_NEW_TAB: &str = "new_tab";
pub const ON_CONNECT_NONE: &str = "none";
pub const CONFIG_TRUE: &str = "true";
pub const CONFIG_FALSE: &str = "false";

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
    /// Put `client` on `target` and bind it there: it's detached when its tab closes, and with
    /// `pin` it's sent back to its tab whenever it wanders off.
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

/// The tab name without the suffix zellij appends to a single-pane tab whose held pane has
/// exited: ` [ EXITED ] ` or ` [ EXIT CODE: n ] ` (zellij-server 0.45.1, `tab/mod.rs`
/// `single_pane_tab_name`). Names are compared stripped, on both sides of the wire.
#[must_use]
pub fn base_tab_name(name: &str) -> &str {
    if let Some(base) = name.strip_suffix(" [ EXITED ] ") {
        return base;
    }
    if let Some(head) = name.strip_suffix(" ] ") {
        if let Some((base, code)) = head.rsplit_once(" [ EXIT CODE: ") {
            if code.parse::<i32>().is_ok() {
                return base;
            }
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn every_permission_name_is_one_zellij_knows() {
        for name in PERMISSIONS {
            assert!(
                zellij_utils::data::PermissionType::from_str(name).is_ok(),
                "zellij 0.45.1 has no permission named {name}"
            );
        }
    }

    #[test]
    fn base_tab_name_strips_the_exit_suffix() {
        assert_eq!(base_tab_name("logs [ EXITED ] "), "logs");
        assert_eq!(base_tab_name("logs [ EXIT CODE: 1 ] "), "logs");
        assert_eq!(base_tab_name("logs [ EXIT CODE: -9 ] "), "logs");
        assert_eq!(base_tab_name("a [ EXITED ]  [ EXITED ] "), "a [ EXITED ] ");
    }

    #[test]
    fn base_tab_name_leaves_other_names_alone() {
        assert_eq!(base_tab_name("logs"), "logs");
        assert_eq!(base_tab_name(""), "");
        assert_eq!(base_tab_name("[ EXITED ]"), "[ EXITED ]");
        assert_eq!(base_tab_name("x [ EXIT CODE: ? ] "), "x [ EXIT CODE: ? ] ");
    }

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

//! The warpify zellij plugin: answers `warpify-proto` requests arriving on the `warpify` pipe.

use std::collections::{BTreeMap, BTreeSet};

use warpify_proto::{Client, Event as WireEvent, Request, State, Tab, HEARTBEAT_SECS, PIPE_NAME};
use zellij_tile::prelude::*;

/// zellij runs one instance of this plugin per connected client and fans every pipe message out
/// to all of them; an instance's tab switches move only its own client.
#[derive(Default)]
struct Warpify {
    /// The client this instance belongs to.
    own_client: ClientId,
    tabs: Vec<TabInfo>,
    panes: PaneManifest,
    clients: Vec<ClientInfo>,
    /// CLI pipes held open by `watch`, by pipe id. zellij gives no signal when a CLI watcher goes
    /// away, so ids of dead pipes stay here (graph @nick/warpify, node #11).
    watchers: BTreeSet<String>,
    /// Last state sent to watchers, to send only on change.
    last_sent: Option<State>,
}

register_plugin!(Warpify);

impl ZellijPlugin for Warpify {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::ReadCliPipes,
        ]);
        subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::ListClients,
            EventType::Timer,
            EventType::PermissionRequestResult,
        ]);
        set_selectable(false);
        self.own_client = get_plugin_ids().client_id;
        set_timeout(HEARTBEAT_SECS);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::TabUpdate(tabs) => {
                self.tabs = tabs;
                list_clients();
            }
            Event::PaneUpdate(panes) => {
                self.panes = panes;
                list_clients();
            }
            Event::ListClients(clients) => {
                self.clients = clients;
                self.broadcast_if_changed();
            }
            Event::PermissionRequestResult(_) => list_clients(),
            Event::Timer(_) => {
                self.send_to_watchers(&WireEvent::Heartbeat);
                // Clients aren't pushed by zellij; the heartbeat doubles as their poll.
                list_clients();
                set_timeout(HEARTBEAT_SECS);
            }
            _ => {}
        }
        false
    }

    fn pipe(&mut self, message: PipeMessage) -> bool {
        if message.name != PIPE_NAME {
            return false;
        }
        let PipeSource::Cli(pipe_id) = message.source else {
            return false;
        };
        let Some(payload) = message.payload else {
            // The CLI side closed the pipe.
            self.watchers.remove(&pipe_id);
            return false;
        };
        let request = serde_json::from_str::<Request>(&payload);
        // A bind is the named client's own instance's job; anything else is answered once, by the
        // leader, or the CLI would get a copy per client.
        let mine = match &request {
            Ok(Request::Bind { client, .. }) => *client == self.own_client,
            _ => self.is_leader(),
        };
        if !mine {
            return false;
        }
        match request {
            Ok(Request::State) => send(&pipe_id, &WireEvent::State(self.state())),
            Ok(Request::Watch) => {
                block_cli_pipe_input(&pipe_id);
                let state = self.state();
                send(&pipe_id, &WireEvent::State(state.clone()));
                self.last_sent = Some(state);
                self.watchers.insert(pipe_id);
            }
            // Not implemented yet; design in graph @nick/warpify, node #9 (risk #10).
            Ok(Request::Bind { .. }) => eprintln!("warpify: bind is not implemented yet"),
            Err(err) => eprintln!("warpify: bad request {payload:?}: {err}"),
        }
        false
    }
}

impl Warpify {
    /// The instance of the lowest connected client speaks for the session. With no client list
    /// yet every instance does: a duplicate reply beats a CLI left hanging.
    fn is_leader(&self) -> bool {
        self.clients
            .iter()
            .map(|c| c.client_id)
            .min()
            .is_none_or(|leader| leader == self.own_client)
    }

    fn state(&self) -> State {
        let tabs = self
            .tabs
            .iter()
            .map(|t| Tab {
                id: t.tab_id,
                position: t.position,
                name: t.name.clone(),
            })
            .collect();
        let clients = self
            .clients
            .iter()
            .filter_map(|c| {
                Some(Client {
                    id: c.client_id,
                    tab: self.tab_of_pane(&c.pane_id)?,
                    managed: false,
                })
            })
            .collect();
        State { tabs, clients }
    }

    /// The stable id of the tab holding `pane`; the manifest is keyed by tab position.
    fn tab_of_pane(&self, pane: &PaneId) -> Option<usize> {
        let (id, is_plugin) = match *pane {
            PaneId::Terminal(id) => (id, false),
            PaneId::Plugin(id) => (id, true),
        };
        let position = self.panes.panes.iter().find_map(|(position, panes)| {
            panes
                .iter()
                .any(|p| p.id == id && p.is_plugin == is_plugin)
                .then_some(*position)
        })?;
        self.tabs
            .iter()
            .find(|t| t.position == position)
            .map(|t| t.tab_id)
    }

    fn broadcast_if_changed(&mut self) {
        if self.watchers.is_empty() {
            return;
        }
        let state = self.state();
        if self.last_sent.as_ref() == Some(&state) {
            return;
        }
        self.send_to_watchers(&WireEvent::State(state.clone()));
        self.last_sent = Some(state);
    }

    fn send_to_watchers(&self, event: &WireEvent) {
        for pipe_id in &self.watchers {
            send(pipe_id, event);
        }
    }
}

fn send(pipe_id: &str, event: &WireEvent) {
    match serde_json::to_string(event) {
        Ok(line) => cli_pipe_output(pipe_id, &format!("{line}\n")),
        Err(err) => eprintln!("warpify: can't encode {event:?}: {err}"),
    }
}

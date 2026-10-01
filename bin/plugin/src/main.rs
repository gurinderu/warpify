//! The warpify zellij plugin: answers `warpify-proto` requests arriving on the `warpify` pipe.

use std::collections::{BTreeMap, BTreeSet};

use warpify_proto::{Event as WireEvent, Request, State, HEARTBEAT_SECS, PIPE_NAME};
use warpify_session::{Snapshot, TabSnapshot};
use zellij_tile::prelude::*;

/// zellij runs one instance of this plugin per connected client and fans every pipe message out
/// to all of them; an instance's tab switches move only its own client.
#[derive(Default)]
struct Warpify {
    session: Snapshot,
    /// CLI pipes held open by `watch`, by pipe id. zellij gives no signal when a CLI watcher goes
    /// away, so ids of dead pipes stay here (graph @nick/warpify, node #11).
    watchers: BTreeSet<String>,
    /// Last state sent to watchers, to send only on change.
    last_sent: Option<State>,
}

register_plugin!(Warpify);

impl ZellijPlugin for Warpify {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        warpify_telemetry::init("warpify=info");
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::ReadCliPipes,
        ]);
        subscribe(&[
            EventType::TabUpdate,
            EventType::Timer,
            EventType::PermissionRequestResult,
        ]);
        set_selectable(false);
        self.session.own_client = get_plugin_ids().client_id;
        set_timeout(HEARTBEAT_SECS);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::TabUpdate(tabs) => {
                self.session.tabs = tabs.iter().map(tab_snapshot).collect();
                self.broadcast_if_changed();
            }
            Event::Timer(_) => {
                self.send_to_watchers(&WireEvent::Heartbeat);
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
        tracing::debug!(pipe_id, ?request, "request received");
        let mine = self.session.handles(request.as_ref().ok());
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
            Ok(Request::Bind { .. }) => tracing::info!(pipe_id, "bind is not implemented yet"),
            Err(err) => tracing::warn!(%err, payload, "bad request"),
        }
        false
    }
}

impl Warpify {
    fn state(&self) -> State {
        self.session.state()
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
            tracing::debug!(pipe_id, ?event, "broadcast");
            send(pipe_id, event);
        }
    }
}

fn send(pipe_id: &str, event: &WireEvent) {
    match serde_json::to_string(event) {
        Ok(line) => cli_pipe_output(pipe_id, &format!("{line}\n")),
        Err(err) => tracing::error!(%err, ?event, "can't encode event"),
    }
}

fn tab_snapshot(tab: &TabInfo) -> TabSnapshot {
    TabSnapshot {
        id: tab.tab_id,
        position: tab.position,
        name: tab.name.clone(),
        active: tab.active,
        other_clients: tab.other_focused_clients.clone(),
    }
}

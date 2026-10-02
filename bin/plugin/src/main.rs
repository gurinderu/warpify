//! The warpify zellij plugin: answers `warpify-proto` requests arriving on the `warpify` pipe.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use warpify_proto::ClientId;
use warpify_proto::{Event as WireEvent, Request, State, TabId, HEARTBEAT_SECS, PIPE_NAME};
use warpify_session::{
    departed, on_tabs, plan_bind, tab_index, BindStep, Config, ConnectAction, Correction, Internal,
    Snapshot, TabSnapshot, INTERNAL_PIPE,
};
use zellij_tile::prelude::*;

/// zellij runs one instance of this plugin per connected client and fans every pipe message out
/// to all of them; an instance's tab switches move only its own client.
#[derive(Default)]
struct Warpify {
    /// The `load_plugins` child block (graph @nick/warpify, node #16).
    config: Config,
    session: Snapshot,
    /// CLI pipes held open by `watch`, by pipe id. zellij gives no signal when a CLI watcher goes
    /// away, so ids of dead pipes stay here (graph @nick/warpify, node #11).
    watchers: BTreeSet<String>,
    /// Last state sent to watchers, to send only on change.
    last_sent: Option<State>,
    /// Clients of the previous `TabUpdate`, to notice departures (graph @nick/warpify, node #9,
    /// risk #12).
    known_clients: BTreeSet<ClientId>,
}

register_plugin!(Warpify);

/// What `load` requests: the names in `warpify_proto::PERMISSIONS`, parsed by zellij's own
/// `PermissionType::from_str`. The installer pre-grants the same names; an unknown one is logged
/// and skipped.
fn requested_permissions() -> Vec<PermissionType> {
    warpify_proto::PERMISSIONS
        .iter()
        .filter_map(|name| {
            let parsed = PermissionType::from_str(name);
            if parsed.is_err() {
                tracing::error!(name, "unknown zellij permission in PERMISSIONS, skipping");
            }
            parsed.ok()
        })
        .collect()
}

impl ZellijPlugin for Warpify {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        warpify_telemetry::init("warpify=info");
        let (config, warnings) = Config::parse(&configuration);
        for warning in warnings {
            tracing::warn!("{warning}");
        }
        self.config = config;
        request_permission(&requested_permissions());
        subscribe(&[
            EventType::TabUpdate,
            // Reaches frozen instances too; reports 0 connected clients once the last client
            // has left (zellij-server `screen.rs` `remove_client`).
            EventType::SessionUpdate,
            // Confirms a zero in `SessionUpdate`; the reply is addressed by (plugin, client) and
            // reaches a departed client's instance (zellij-server `plugins/mod.rs`
            // `ListClientsToPlugin`).
            EventType::ListClients,
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
                self.session
                    .apply_tabs(tabs.iter().map(tab_snapshot).collect());
                self.serve_connect();
                self.announce_departures();
                self.correct_binding();
                self.broadcast_if_changed();
            }
            Event::SessionUpdate(sessions, _) => {
                let empty = sessions
                    .iter()
                    .any(|s| s.is_current_session && s.connected_clients == 0);
                // A disk-scan SessionUpdate may carry a stale zero: the client list confirms it.
                if empty && self.session.on_zero_clients() {
                    tracing::debug!(
                        client = self.session.own_client,
                        "zero clients reported, confirming"
                    );
                    list_clients();
                }
            }
            Event::ListClients(clients) => {
                if self.session.on_client_list(clients.len()) {
                    tracing::info!(
                        client = self.session.own_client,
                        "no clients connected, freezing"
                    );
                    self.freeze();
                }
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
        if message.name == INTERNAL_PIPE {
            self.on_internal(&message);
            return false;
        }
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
            // graph @nick/warpify, node #9 (risk #10)
            Ok(Request::Bind { target, pin, .. }) => self.bind(&target, pin),
            Err(err) => tracing::warn!(%err, payload, "bad request"),
        }
        false
    }
}

impl Warpify {
    fn state(&self) -> State {
        self.session.state()
    }

    /// Tell every instance of this plugin about clients that were in the previous update and are
    /// gone now. zellij keeps a departed client's instance alive, with its binding, and hands it
    /// to the next client with that id, and it signals no disconnects: the other instances are
    /// the ones who see it. Duplicates from several instances are harmless (graph @nick/warpify,
    /// node #9, risk #12). No destination: zellij fans the message out to every plugin instance in
    /// the session, launching nothing (zellij-server `plugins/mod.rs`, `MessageFromPlugin`).
    fn announce_departures(&mut self) {
        let now = self.session.client_ids();
        for client in departed(&self.known_clients, &now) {
            tracing::info!(client, "client departed, telling instances to forget it");
            let message = Internal::Forget { client };
            pipe_message_to_plugin(
                MessageToPlugin::new(INTERNAL_PIPE).with_payload(message.encode()),
            );
        }
        self.known_clients = now;
    }

    fn on_internal(&mut self, message: &PipeMessage) {
        if !matches!(message.source, PipeSource::Plugin(_)) {
            return;
        }
        let Some(payload) = &message.payload else {
            return;
        };
        match Internal::decode(payload) {
            Ok(Internal::Forget { client }) => {
                if client == self.session.own_client && self.session.connected {
                    tracing::info!(client, "client departed, freezing");
                    self.freeze();
                }
            }
            Err(err) => tracing::warn!(%err, payload, "bad internal message"),
        }
    }

    /// `on_connect`: bind a newly connected client, from its first tab update after `load` or the
    /// one that un-freezes the instance (graph @nick/warpify, node #16).
    fn serve_connect(&mut self) {
        let client = self.session.own_client;
        match self.session.on_connect(&self.config) {
            Some(ConnectAction::BoundCurrent(tab)) => {
                tracing::info!(
                    client,
                    tab,
                    pin = self.config.pin,
                    "on connect: bound the tab it is on"
                );
            }
            Some(ConnectAction::NewTab { pin }) => {
                tracing::info!(client, pin, "on connect: new tab");
                self.bind(&warpify_proto::Target::New(None), pin);
            }
            None => {}
        }
    }

    /// Go silent: the client is gone, so its binding and the watchers held here are dead.
    fn freeze(&mut self) {
        self.session.freeze();
        self.watchers.clear();
        self.last_sent = None;
    }

    fn bind(&mut self, target: &warpify_proto::Target, pin: bool) {
        let client = self.session.own_client;
        let step = plan_bind(target, &self.session.tabs);
        let tab = match &step {
            BindStep::GoTo(id) => {
                go_to_tab_id(&self.session.tabs, *id);
                Some(*id)
            }
            BindStep::FocusOrCreate(name) => focus_or_create_tab(name),
            BindStep::CreateNew(name) => new_tab(name.as_deref(), None),
            BindStep::Fail(reason) => {
                tracing::warn!(client, reason, "bind failed");
                return;
            }
        };
        self.session.finish_bind(&step, pin, tab);
        if let Some(tab) = tab {
            tracing::info!(client, tab, pin, "bind executed");
        } else {
            tracing::warn!(
                client,
                ?target,
                "zellij returned no tab for bind, waiting for the tab update"
            );
        }
    }

    /// Undo a drift from the binding, once the binding is in force (graph @nick/warpify, node #9).
    fn correct_binding(&mut self) {
        let client = self.session.own_client;
        let Some(binding) = self.session.binding else {
            return;
        };
        let binding = binding.seen_in(&self.session.tabs);
        self.session.binding = Some(binding);
        match on_tabs(self.session.binding, &self.session) {
            Some(Correction::GoTo(tab)) => {
                tracing::info!(client, tab, pin = binding.pin, "pin correction");
                go_to_tab_id(&self.session.tabs, tab);
            }
            Some(Correction::Detach) => {
                tracing::info!(
                    client,
                    tab = binding.tab,
                    pin = binding.pin,
                    "bound tab gone, detaching"
                );
                self.session.binding = None;
                detach();
            }
            None => {}
        }
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

/// Move the own client to tab `id` by its index in the current snapshot; no `run_action`, which
/// would need `RunActionsAsUser` (graph @nick/warpify, node #9).
fn go_to_tab_id(tabs: &[TabSnapshot], id: TabId) {
    if let Some(index) = tab_index(tabs, id) {
        switch_tab_to(index);
    } else {
        tracing::warn!(id, "no such tab to go to");
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

//! The plugin's session logic, free of zellij types so it builds and tests on the host: the
//! plugin converts what zellij reports into these snapshots and asks them what to answer.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use warpify_proto::{Client, ClientId, Request, State, Tab, TabId, Target};

/// A tab as one plugin instance sees it in its own `TabUpdate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabSnapshot {
    pub id: TabId,
    pub position: usize,
    pub name: String,
    /// The instance's own client is on this tab.
    pub active: bool,
    /// Every other connected client whose active tab is this one.
    pub other_clients: Vec<ClientId>,
}

/// What one plugin instance currently knows about the session.
///
/// Requires a non-mirrored session: in a mirrored one zellij reports no other clients, so only
/// the instance's own client would show up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// The client this instance belongs to.
    pub own_client: ClientId,
    pub tabs: Vec<TabSnapshot>,
    /// Set while this instance's client is bound (graph @nick/warpify, node #9).
    pub binding: Option<Binding>,
}

/// A client held on a tab by its own plugin instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub tab: TabId,
    /// Keep the client on the tab: switching away is undone.
    pub pin: bool,
    /// The instance's own client has been seen on the bound tab. Until then the binding is not
    /// in force: a bind creates tabs asynchronously and may fail, so neither a missing tab nor
    /// a client elsewhere is evidence about the binding (graph @nick/warpify, node #9).
    pub seen: bool,
}

impl Binding {
    /// A fresh binding on `tab`, not in force yet.
    #[must_use]
    pub fn new(tab: TabId, pin: bool) -> Self {
        Self {
            tab,
            pin,
            seen: false,
        }
    }

    /// This binding, marked seen if the own client is on its tab in `tabs`.
    #[must_use]
    pub fn seen_in(self, tabs: &[TabSnapshot]) -> Self {
        Self {
            seen: self.seen || tabs.iter().any(|t| t.active && t.id == self.tab),
            ..self
        }
    }
}

/// What the instance does to carry out a bind request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindStep {
    GoTo(TabId),
    FocusOrCreate(String),
    CreateNew(Option<String>),
    Fail(String),
}

/// What the instance does to keep a binding true after a tab update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Correction {
    /// The bound tab is gone: detach and drop the binding.
    Detach,
    GoTo(TabId),
}

/// The first step of a bind: resolve `target` against the known tabs.
#[must_use]
pub fn plan_bind(target: &Target, tabs: &[TabSnapshot]) -> BindStep {
    match target {
        Target::Id(id) if tabs.iter().any(|t| t.id == *id) => BindStep::GoTo(*id),
        Target::Id(id) => BindStep::Fail(format!("no tab with id {id}")),
        Target::Name(name) => tabs.iter().find(|t| t.name == *name).map_or_else(
            || BindStep::FocusOrCreate(name.clone()),
            |t| BindStep::GoTo(t.id),
        ),
        Target::New(name) => BindStep::CreateNew(name.clone()),
    }
}

/// What a bound instance must do after a tab update; `None` when nothing is wrong.
#[must_use]
pub fn on_tabs(binding: Option<Binding>, snapshot: &Snapshot) -> Option<Correction> {
    let binding = binding?.seen_in(&snapshot.tabs);
    if !binding.seen {
        return None;
    }
    if snapshot.tabs.iter().all(|t| t.id != binding.tab) {
        return Some(Correction::Detach);
    }
    let on_bound = snapshot
        .tabs
        .iter()
        .any(|t| t.active && t.id == binding.tab);
    (binding.pin && !on_bound).then_some(Correction::GoTo(binding.tab))
}

/// The 1-based index `switch_tab_to` takes for the tab `id`: zellij's `GoToTab` does
/// `index.saturating_sub(1)` and then looks the tab up by position (zellij-server 0.45.1,
/// `screen.rs` `go_to_tab`), so index = position + 1. `None` if the tab isn't listed
/// (graph @nick/warpify, node #9).
#[must_use]
pub fn tab_index(tabs: &[TabSnapshot], id: TabId) -> Option<u32> {
    let position = tabs.iter().find(|t| t.id == id)?.position;
    u32::try_from(position).ok()?.checked_add(1)
}

/// Pipe name of the plugin-to-plugin messages between instances.
pub const INTERNAL_PIPE: &str = "warpify-internal";

/// A message one instance sends to all instances of the plugin; kept out of the public proto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "msg", rename_all = "snake_case")]
pub enum Internal {
    /// `client` has left the session: the instance that belongs to it drops its binding, since
    /// zellij keeps that instance and hands it to the next client with the same id (graph
    /// @nick/warpify, node #9, risk #12).
    Forget { client: ClientId },
}

impl Internal {
    /// The message as the pipe payload.
    ///
    /// # Panics
    /// Never: the type serializes infallibly.
    #[must_use]
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("plain enum serializes")
    }

    /// # Errors
    /// The payload isn't an internal message.
    pub fn decode(payload: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(payload)
    }
}

/// The clients that were in `before` and are not in `now`: they have disconnected.
#[must_use]
pub fn departed(before: &BTreeSet<ClientId>, now: &BTreeSet<ClientId>) -> Vec<ClientId> {
    before.difference(now).copied().collect()
}

impl Snapshot {
    /// The wire state, clients derived from the tabs.
    #[must_use]
    pub fn state(&self) -> State {
        let tabs = self
            .tabs
            .iter()
            .map(|t| Tab {
                id: t.id,
                position: t.position,
                name: t.name.clone(),
            })
            .collect();
        State {
            tabs,
            clients: self.clients(),
        }
    }

    /// Connected clients with their tabs: the own client is on the active tab, the others on the
    /// tab listing them. A client listed twice is reported once, on the first tab.
    fn clients(&self) -> Vec<Client> {
        let mut clients: Vec<Client> = Vec::new();
        for tab in &self.tabs {
            let own = tab.active.then_some(self.own_client);
            for id in own.into_iter().chain(tab.other_clients.iter().copied()) {
                if clients.iter().all(|c| c.id != id) {
                    clients.push(Client { id, tab: tab.id });
                }
            }
        }
        clients
    }

    /// Ids of the connected clients, own included.
    #[must_use]
    pub fn client_ids(&self) -> BTreeSet<ClientId> {
        self.clients().iter().map(|c| c.id).collect()
    }

    /// Apply `Internal::Forget`: only the instance of that client drops its binding.
    pub fn forget(&mut self, client: ClientId) {
        if client == self.own_client {
            self.binding = None;
        }
    }

    /// The instance of the lowest connected client speaks for the session. With no client known
    /// yet every instance does: a duplicate reply beats a CLI left hanging.
    #[must_use]
    pub fn is_leader(&self) -> bool {
        self.clients()
            .iter()
            .map(|c| c.id)
            .min()
            .is_none_or(|leader| leader == self.own_client)
    }

    /// Whether this instance handles `request` (`None`: it didn't parse). A bind is the named
    /// client's own instance's job; anything else is answered once, by the leader, or the CLI
    /// would get a copy per client.
    #[must_use]
    pub fn handles(&self, request: Option<&Request>) -> bool {
        match request {
            Some(Request::Bind { client, .. }) => *client == self.own_client,
            _ => self.is_leader(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use warpify_proto::Target;

    fn tab(id: TabId, position: usize, active: bool, others: &[ClientId]) -> TabSnapshot {
        TabSnapshot {
            id,
            position,
            name: format!("t{id}"),
            active,
            other_clients: others.to_vec(),
        }
    }

    fn client(id: ClientId, tab: TabId) -> Client {
        Client { id, tab }
    }

    /// A snapshot for `own_client` where it and `others` share one tab.
    fn with_clients(own_client: ClientId, others: &[ClientId]) -> Snapshot {
        Snapshot {
            own_client,
            tabs: vec![tab(0, 0, true, others)],
            binding: None,
        }
    }

    #[test]
    fn own_client_is_on_the_active_tab() {
        let snap = Snapshot {
            own_client: 3,
            tabs: vec![tab(7, 0, false, &[]), tab(9, 1, true, &[])],
            ..Snapshot::default()
        };
        assert_eq!(snap.state().tabs.len(), 2);
        assert_eq!(snap.state().clients, vec![client(3, 9)]);
    }

    #[test]
    fn other_clients_are_on_the_tab_listing_them() {
        let snap = Snapshot {
            own_client: 1,
            tabs: vec![tab(7, 0, true, &[4]), tab(9, 1, false, &[2, 5])],
            ..Snapshot::default()
        };
        assert_eq!(
            snap.state().clients,
            vec![client(1, 7), client(4, 7), client(2, 9), client(5, 9)]
        );
    }

    #[test]
    fn client_listed_on_two_tabs_is_reported_once() {
        let snap = Snapshot {
            own_client: 1,
            tabs: vec![tab(7, 0, true, &[2]), tab(9, 1, false, &[2, 1])],
            ..Snapshot::default()
        };
        assert_eq!(snap.state().clients, vec![client(1, 7), client(2, 7)]);
    }

    #[test]
    fn no_tabs_no_clients_and_this_instance_leads() {
        let snap = Snapshot {
            own_client: 3,
            tabs: vec![],
            binding: None,
        };
        assert_eq!(snap.state().clients, vec![]);
        assert!(snap.is_leader());
    }

    #[test]
    fn leader_is_the_lowest_client() {
        assert!(with_clients(2, &[5, 8]).is_leader());
        assert!(!with_clients(5, &[2, 8]).is_leader());
        assert!(!with_clients(8, &[5, 2]).is_leader());
    }

    #[test]
    fn bind_goes_only_to_the_named_clients_instance() {
        let bind = Request::Bind {
            client: 2,
            target: Target::Id(0),
            pin: false,
        };
        // Client 1 is the leader, yet a bind for client 2 is not its job.
        assert!(!with_clients(1, &[2]).handles(Some(&bind)));
        assert!(with_clients(2, &[1]).handles(Some(&bind)));
    }

    #[test]
    fn other_requests_go_to_the_leader() {
        for request in [Some(&Request::State), Some(&Request::Watch), None] {
            assert!(with_clients(1, &[2]).handles(request));
            assert!(!with_clients(2, &[1]).handles(request));
        }
    }

    fn named(id: TabId, name: &str, active: bool) -> TabSnapshot {
        TabSnapshot {
            name: name.into(),
            ..tab(id, id, active, &[])
        }
    }

    /// A binding already in force (own client seen on its tab).
    fn bound(tab: TabId, pin: bool, tabs: Vec<TabSnapshot>) -> (Option<Binding>, Snapshot) {
        let binding = Some(Binding {
            tab,
            pin,
            seen: true,
        });
        (
            binding,
            Snapshot {
                own_client: 1,
                tabs,
                binding,
            },
        )
    }

    #[test]
    fn bind_to_existing_id_goes_there() {
        let tabs = [named(3, "a", false)];
        assert_eq!(plan_bind(&Target::Id(3), &tabs), BindStep::GoTo(3));
    }

    #[test]
    fn bind_to_missing_id_fails() {
        let tabs = [named(3, "a", false)];
        assert_eq!(
            plan_bind(&Target::Id(4), &tabs),
            BindStep::Fail("no tab with id 4".into())
        );
    }

    #[test]
    fn bind_to_existing_name_goes_to_its_id() {
        let tabs = [named(3, "a", false), named(5, "b", false)];
        assert_eq!(
            plan_bind(&Target::Name("b".into()), &tabs),
            BindStep::GoTo(5)
        );
    }

    #[test]
    fn bind_to_unknown_name_focuses_or_creates() {
        let tabs = [named(3, "a", false)];
        assert_eq!(
            plan_bind(&Target::Name("z".into()), &tabs),
            BindStep::FocusOrCreate("z".into())
        );
    }

    #[test]
    fn bind_to_new_creates_even_if_the_name_exists() {
        let tabs = [named(3, "a", false)];
        assert_eq!(
            plan_bind(&Target::New(Some("a".into())), &tabs),
            BindStep::CreateNew(Some("a".into()))
        );
        assert_eq!(
            plan_bind(&Target::New(None), &[]),
            BindStep::CreateNew(None)
        );
    }

    #[test]
    fn unmanaged_instance_needs_no_correction() {
        let snap = with_clients(1, &[]);
        assert_eq!(on_tabs(None, &snap), None);
    }

    #[test]
    fn vanished_bound_tab_detaches() {
        for pin in [false, true] {
            let (b, snap) = bound(9, pin, vec![named(3, "a", true)]);
            assert_eq!(on_tabs(b, &snap), Some(Correction::Detach));
        }
    }

    #[test]
    fn pinned_client_pulled_back_to_the_bound_tab() {
        let (b, snap) = bound(9, true, vec![named(3, "a", true), named(9, "b", false)]);
        assert_eq!(on_tabs(b, &snap), Some(Correction::GoTo(9)));
    }

    #[test]
    fn pinned_client_on_the_bound_tab_is_left_alone() {
        let (b, snap) = bound(9, true, vec![named(3, "a", false), named(9, "b", true)]);
        assert_eq!(on_tabs(b, &snap), None);
    }

    #[test]
    fn unpinned_client_may_wander() {
        let (b, snap) = bound(9, false, vec![named(3, "a", true), named(9, "b", false)]);
        assert_eq!(on_tabs(b, &snap), None);
    }

    #[test]
    fn unseen_missing_tab_waits() {
        for pin in [false, true] {
            let snap = Snapshot {
                own_client: 1,
                tabs: vec![named(3, "a", true)],
                binding: None,
            };
            assert_eq!(on_tabs(Some(Binding::new(9, pin)), &snap), None);
        }
    }

    #[test]
    fn own_client_arriving_on_the_bound_tab_marks_seen() {
        let b = Binding::new(9, false);
        let elsewhere = [named(3, "a", true), named(9, "b", false)];
        let arrived = [named(3, "a", false), named(9, "b", true)];
        assert!(!b.seen_in(&elsewhere).seen);
        assert!(b.seen_in(&arrived).seen);
        // Seen stays seen when the client leaves or the tab vanishes.
        assert!(b.seen_in(&arrived).seen_in(&elsewhere).seen);
        assert!(b.seen_in(&arrived).seen_in(&elsewhere[..1]).seen);
    }

    #[test]
    fn pin_pulls_back_only_once_seen() {
        let snap = Snapshot {
            own_client: 1,
            tabs: vec![named(3, "a", true), named(9, "b", false)],
            binding: None,
        };
        // The tab exists but the client never reached it: not in force, no pull-back.
        assert_eq!(on_tabs(Some(Binding::new(9, true)), &snap), None);
        let seen = Binding {
            seen: true,
            ..Binding::new(9, true)
        };
        assert_eq!(on_tabs(Some(seen), &snap), Some(Correction::GoTo(9)));
    }

    #[test]
    fn seen_binding_with_missing_tab_detaches() {
        let snap = Snapshot {
            own_client: 1,
            tabs: vec![named(3, "a", true)],
            binding: None,
        };
        let seen = Binding {
            seen: true,
            ..Binding::new(9, false)
        };
        assert_eq!(on_tabs(Some(seen), &snap), Some(Correction::Detach));
    }

    #[test]
    fn tab_index_is_position_plus_one() {
        let tabs = [
            TabSnapshot {
                position: 0,
                ..named(7, "a", true)
            },
            TabSnapshot {
                position: 1,
                ..named(3, "b", false)
            },
        ];
        assert_eq!(tab_index(&tabs, 7), Some(1));
        assert_eq!(tab_index(&tabs, 3), Some(2));
        assert_eq!(tab_index(&tabs, 9), None);
    }

    #[test]
    fn departed_are_the_ids_that_vanished() {
        let set = |ids: &[ClientId]| ids.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(departed(&set(&[1, 2, 3]), &set(&[1, 3, 4])), vec![2]);
        assert_eq!(departed(&set(&[]), &set(&[1])), Vec::<ClientId>::new());
        assert_eq!(departed(&set(&[1, 2]), &set(&[])), vec![1, 2]);
    }

    #[test]
    fn client_ids_cover_own_and_others() {
        let snap = Snapshot {
            own_client: 1,
            tabs: vec![tab(7, 0, true, &[4]), tab(9, 1, false, &[2])],
            ..Snapshot::default()
        };
        assert_eq!(snap.client_ids(), BTreeSet::from([1, 2, 4]));
    }

    #[test]
    fn forget_clears_only_the_named_clients_binding() {
        let (_, mut snap) = bound(7, true, vec![tab(7, 0, true, &[])]);
        snap.forget(2);
        assert!(snap.binding.is_some());
        snap.forget(1);
        assert_eq!(snap.binding, None);
    }

    #[test]
    fn internal_message_round_trips() {
        let msg = Internal::Forget { client: 5 };
        assert_eq!(msg.encode(), r#"{"msg":"forget","client":5}"#);
        assert_eq!(Internal::decode(&msg.encode()).unwrap(), msg);
        assert!(Internal::decode("{}").is_err());
    }
}

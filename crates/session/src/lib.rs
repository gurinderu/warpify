//! The plugin's session logic, free of zellij types so it builds and tests on the host: the
//! plugin converts what zellij reports into these snapshots and asks them what to answer.

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
    /// The bound tab has appeared in a `TabUpdate`. A bind creates tabs asynchronously, so an
    /// update queued before the creation can arrive after it; until the tab is seen, its absence
    /// is not evidence that it is gone.
    pub seen: bool,
}

impl Binding {
    /// A fresh binding on `tab`, not seen yet.
    #[must_use]
    pub fn new(tab: TabId, pin: bool) -> Self {
        Self {
            tab,
            pin,
            seen: false,
        }
    }

    /// This binding, marked seen if `tabs` lists its tab.
    #[must_use]
    pub fn seen_in(self, tabs: &[TabSnapshot]) -> Self {
        Self {
            seen: self.seen || tabs.iter().any(|t| t.id == self.tab),
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
    /// tab listing them. Only the own client can be `managed`: this instance knows no other's
    /// binding. A client listed twice is reported once, on the first tab.
    fn clients(&self) -> Vec<Client> {
        let mut clients: Vec<Client> = Vec::new();
        for tab in &self.tabs {
            let own = tab.active.then_some(self.own_client);
            for id in own.into_iter().chain(tab.other_clients.iter().copied()) {
                if clients.iter().all(|c| c.id != id) {
                    clients.push(Client {
                        id,
                        tab: tab.id,
                        managed: id == self.own_client && self.binding.is_some(),
                    });
                }
            }
        }
        clients
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
        Client {
            id,
            tab,
            managed: false,
        }
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
    fn only_the_own_bound_client_is_managed() {
        let (_, mut snap) = bound(7, false, vec![tab(7, 0, true, &[4])]);
        assert!(
            snap.state()
                .clients
                .iter()
                .find(|c| c.id == 1)
                .unwrap()
                .managed
        );
        assert!(
            !snap
                .state()
                .clients
                .iter()
                .find(|c| c.id == 4)
                .unwrap()
                .managed
        );
        snap.binding = None;
        assert!(snap.state().clients.iter().all(|c| !c.managed));
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
    fn first_appearance_marks_seen() {
        let tabs = [named(3, "a", true), named(9, "b", false)];
        let b = Binding::new(9, false);
        assert!(!b.seen_in(&tabs[..1]).seen);
        assert!(b.seen_in(&tabs).seen);
        // Seen stays seen when the tab vanishes.
        assert!(b.seen_in(&tabs).seen_in(&tabs[..1]).seen);
    }

    #[test]
    fn pin_pulls_back_only_once_seen() {
        let snap = Snapshot {
            own_client: 1,
            tabs: vec![named(3, "a", true), named(9, "b", false)],
            binding: None,
        };
        // Tab 9 is listed now, so this very update marks it seen and pulls back.
        assert_eq!(
            on_tabs(Some(Binding::new(9, true)), &snap),
            Some(Correction::GoTo(9))
        );
        let before = Snapshot {
            tabs: vec![named(3, "a", true)],
            ..snap
        };
        assert_eq!(on_tabs(Some(Binding::new(9, true)), &before), None);
    }
}

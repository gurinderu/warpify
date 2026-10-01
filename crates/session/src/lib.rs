//! The plugin's session logic, free of zellij types so it builds and tests on the host: the
//! plugin converts what zellij reports into these snapshots and asks them what to answer.

use warpify_proto::{Client, ClientId, Request, State, Tab, TabId};

/// A pane as zellij addresses it: terminal and plugin panes number independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneRef {
    Terminal(u32),
    Plugin(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabSnapshot {
    pub id: TabId,
    pub position: usize,
    pub name: String,
}

/// The panes of the tab at `tab_position`; zellij's pane manifest is keyed by position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSnapshot {
    pub tab_position: usize,
    pub panes: Vec<PaneRef>,
}

/// A connected client and the pane it is focused on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientSnapshot {
    pub id: ClientId,
    pub pane: PaneRef,
}

/// What the plugin currently knows about the session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub tabs: Vec<TabSnapshot>,
    pub panes: Vec<PaneSnapshot>,
    pub clients: Vec<ClientSnapshot>,
}

impl Snapshot {
    /// The wire state; a client whose pane is in no known tab is left out.
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
        let clients = self
            .clients
            .iter()
            .filter_map(|c| {
                Some(Client {
                    id: c.id,
                    tab: self.tab_of_pane(c.pane)?,
                    managed: false,
                })
            })
            .collect();
        State { tabs, clients }
    }

    /// The stable id of the tab holding `pane`.
    fn tab_of_pane(&self, pane: PaneRef) -> Option<TabId> {
        let position = self
            .panes
            .iter()
            .find(|p| p.panes.contains(&pane))?
            .tab_position;
        self.tabs
            .iter()
            .find(|t| t.position == position)
            .map(|t| t.id)
    }

    /// The instance of the lowest connected client speaks for the session. With no client list
    /// yet every instance does: a duplicate reply beats a CLI left hanging.
    #[must_use]
    pub fn is_leader(&self, own_client: ClientId) -> bool {
        self.clients
            .iter()
            .map(|c| c.id)
            .min()
            .is_none_or(|leader| leader == own_client)
    }

    /// Whether the instance of `own_client` handles `request` (`None`: it didn't parse). A bind
    /// is the named client's own instance's job; anything else is answered once, by the leader,
    /// or the CLI would get a copy per client.
    #[must_use]
    pub fn handles(&self, own_client: ClientId, request: Option<&Request>) -> bool {
        match request {
            Some(Request::Bind { client, .. }) => *client == own_client,
            _ => self.is_leader(own_client),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use warpify_proto::Target;

    fn tab(id: TabId, position: usize) -> TabSnapshot {
        TabSnapshot {
            id,
            position,
            name: format!("t{id}"),
        }
    }

    fn client(id: ClientId, pane: PaneRef) -> ClientSnapshot {
        ClientSnapshot { id, pane }
    }

    fn with_clients(ids: &[ClientId]) -> Snapshot {
        Snapshot {
            clients: ids
                .iter()
                .map(|&id| client(id, PaneRef::Terminal(0)))
                .collect(),
            ..Snapshot::default()
        }
    }

    #[test]
    fn client_maps_to_the_tab_holding_its_pane() {
        let snap = Snapshot {
            tabs: vec![tab(7, 0), tab(9, 1)],
            panes: vec![
                PaneSnapshot {
                    tab_position: 0,
                    panes: vec![PaneRef::Terminal(1)],
                },
                PaneSnapshot {
                    tab_position: 1,
                    panes: vec![PaneRef::Terminal(2), PaneRef::Terminal(3)],
                },
            ],
            clients: vec![client(1, PaneRef::Terminal(3))],
        };
        let state = snap.state();
        assert_eq!(state.tabs.len(), 2);
        assert_eq!(
            state.clients,
            vec![Client {
                id: 1,
                tab: 9,
                managed: false
            }]
        );
    }

    #[test]
    fn plugin_and_terminal_panes_with_the_same_id_differ() {
        let snap = Snapshot {
            tabs: vec![tab(7, 0), tab(9, 1)],
            panes: vec![
                PaneSnapshot {
                    tab_position: 0,
                    panes: vec![PaneRef::Terminal(4)],
                },
                PaneSnapshot {
                    tab_position: 1,
                    panes: vec![PaneRef::Plugin(4)],
                },
            ],
            clients: vec![
                client(1, PaneRef::Terminal(4)),
                client(2, PaneRef::Plugin(4)),
            ],
        };
        let tabs: Vec<_> = snap.state().clients.iter().map(|c| c.tab).collect();
        assert_eq!(tabs, [7, 9]);
    }

    #[test]
    fn client_with_unknown_pane_is_dropped() {
        let snap = Snapshot {
            tabs: vec![tab(7, 0)],
            panes: vec![PaneSnapshot {
                tab_position: 0,
                panes: vec![PaneRef::Terminal(1)],
            }],
            clients: vec![client(1, PaneRef::Terminal(99))],
        };
        assert!(snap.state().clients.is_empty());
    }

    #[test]
    fn leader_is_the_lowest_client() {
        let snap = with_clients(&[5, 2, 8]);
        assert!(snap.is_leader(2));
        assert!(!snap.is_leader(5));
        assert!(!snap.is_leader(8));
    }

    #[test]
    fn no_clients_every_instance_leads() {
        let snap = Snapshot::default();
        assert!(snap.is_leader(0));
        assert!(snap.is_leader(3));
    }

    #[test]
    fn bind_goes_only_to_the_named_clients_instance() {
        let snap = with_clients(&[1, 2]);
        let bind = Request::Bind {
            client: 2,
            target: Target::Id(0),
            pin: false,
        };
        // Client 1 is the leader, yet a bind for client 2 is not its job.
        assert!(!snap.handles(1, Some(&bind)));
        assert!(snap.handles(2, Some(&bind)));
    }

    #[test]
    fn other_requests_go_to_the_leader() {
        let snap = with_clients(&[1, 2]);
        for request in [Some(&Request::State), Some(&Request::Watch), None] {
            assert!(snap.handles(1, request));
            assert!(!snap.handles(2, request));
        }
    }
}

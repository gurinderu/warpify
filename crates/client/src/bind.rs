//! Binding a client to a tab from outside: the confirm predicate and the polling around `state`
//! (graph @nick/warpify, node #9).

use std::time::{Duration, Instant};

use warpify_proto::{base_tab_name, ClientId, Request, State, Tab, TabId, Target};

use crate::{fetch_state, send, Error, SessionTarget};

const POLL_EVERY: Duration = Duration::from_millis(200);
const CONFIRM_WITHIN: Duration = Duration::from_secs(3);

/// The tab `client` is on if that satisfies `target`: the given id, a tab of the given name, or
/// (`New`) any tab other than `before`, where the client was before the bind.
#[must_use]
pub fn confirmed<'a>(
    state: &'a State,
    client: ClientId,
    target: &Target,
    before: Option<TabId>,
) -> Option<&'a Tab> {
    let tab = state.tab(state.client(client)?.tab)?;
    let ok = match target {
        Target::Id(id) => tab.id == *id,
        Target::Name(name) => base_tab_name(&tab.name) == base_tab_name(name),
        Target::New(_) => Some(tab.id) != before,
    };
    ok.then_some(tab)
}

/// Binds `client` to `target`, then polls `state` until it's there.
///
/// # Errors
/// Whatever fetching state or sending the bind fails with, or [`Error::NotMoved`] after 3 s.
pub fn bind_and_confirm(
    session: &SessionTarget,
    client: ClientId,
    target: &Target,
    pin: bool,
) -> Result<Tab, Error> {
    let before = fetch_state(session)?.client(client).map(|c| c.tab);
    send(
        &Request::Bind {
            client,
            target: target.clone(),
            pin,
        },
        session,
    )?;
    let deadline = Instant::now() + CONFIRM_WITHIN;
    loop {
        match fetch_state(session) {
            Ok(state) => {
                if let Some(tab) = confirmed(&state, client, target, before) {
                    return Ok(tab.clone());
                }
            }
            Err(err) => tracing::debug!(%err, "state poll failed"),
        }
        if Instant::now() >= deadline {
            return Err(Error::NotMoved(client));
        }
        std::thread::sleep(POLL_EVERY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use warpify_proto::Client;

    fn state(tabs: &[(TabId, &str)], clients: &[(ClientId, TabId)]) -> State {
        State {
            tabs: tabs
                .iter()
                .enumerate()
                .map(|(position, (id, name))| Tab {
                    id: *id,
                    position,
                    name: (*name).into(),
                })
                .collect(),
            clients: clients
                .iter()
                .map(|(id, tab)| Client { id: *id, tab: *tab })
                .collect(),
        }
    }

    #[test]
    fn confirm_by_name_ignores_the_exit_suffix() {
        let s = state(&[(0, "logs [ EXITED ] ")], &[(1, 0)]);
        assert!(confirmed(&s, 1, &Target::Name("logs".into()), None).is_some());
    }

    #[test]
    fn confirm_by_tab_id_and_name() {
        let s = state(&[(0, "a"), (4, "logs")], &[(1, 4)]);
        assert_eq!(confirmed(&s, 1, &Target::Id(4), Some(0)).unwrap().id, 4);
        assert!(confirmed(&s, 1, &Target::Id(0), Some(0)).is_none());
        assert!(confirmed(&s, 1, &Target::Name("logs".into()), None).is_some());
        assert!(confirmed(&s, 1, &Target::Name("a".into()), None).is_none());
    }

    #[test]
    fn confirm_new_needs_a_changed_tab() {
        let s = state(&[(0, "a"), (4, "t")], &[(1, 4)]);
        assert!(confirmed(&s, 1, &Target::New(None), Some(0)).is_some());
        assert!(confirmed(&s, 1, &Target::New(None), Some(4)).is_none());
        assert!(confirmed(&s, 1, &Target::New(None), None).is_some());
    }

    #[test]
    fn confirm_needs_the_client() {
        let s = state(&[(0, "a")], &[]);
        assert!(confirmed(&s, 1, &Target::Id(0), None).is_none());
    }

    #[test]
    fn not_moved_message() {
        assert_eq!(
            Error::NotMoved(2).to_string(),
            "bind sent but client 2 didn't move — is it connected?"
        );
    }
}

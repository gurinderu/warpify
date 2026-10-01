//! Binding a client to a tab and `attach`'s helpers: the confirm predicate, picking a freshly
//! connected client, and the polling around `state` (graph @nick/warpify, nodes #9, #16).

use std::time::{Duration, Instant};

use warpify_proto::{ClientId, Request, State, Tab, TabId, Target};

use crate::{fetch_state, send, Error, SessionTarget};

/// The session `attach` uses when none is named.
pub const DEFAULT_SESSION: &str = "warpify";

const POLL_EVERY: Duration = Duration::from_millis(200);
const CONFIRM_WITHIN: Duration = Duration::from_secs(3);
const NEW_CLIENT_WITHIN: Duration = Duration::from_secs(10);

/// The session `attach` targets: the named one, else [`DEFAULT_SESSION`].
#[must_use]
pub fn attach_session(named: Option<&str>) -> &str {
    named.unwrap_or(DEFAULT_SESSION)
}

/// `attach` starts a zellij client, so it can't run from inside a session.
///
/// # Errors
/// [`Error::InsideSession`] if `env_session` (`ZELLIJ_SESSION_NAME`) is set and non-empty.
pub fn check_outside_zellij(env_session: Option<&str>) -> Result<(), Error> {
    match env_session {
        Some(name) if !name.is_empty() => Err(Error::InsideSession(name.to_owned())),
        _ => Ok(()),
    }
}

/// Parses a comma-separated list of client ids; the empty string is the empty list.
///
/// # Errors
/// [`Error::BadKnown`] on an item that isn't a client id.
pub fn parse_known(list: &str) -> Result<Vec<ClientId>, Error> {
    list.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| item.parse().map_err(|_| Error::BadKnown(item.to_owned())))
        .collect()
}

/// The lowest client id in `state` that isn't in `known`.
#[must_use]
pub fn pick_new(state: &State, known: &[ClientId]) -> Option<ClientId> {
    state
        .clients
        .iter()
        .map(|c| c.id)
        .filter(|id| !known.contains(id))
        .min()
}

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
        Target::Name(name) => tab.name == *name,
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

/// Polls `state` for up to 10 s until a client not in `known` shows up and returns the lowest.
///
/// # Errors
/// [`Error::NoNewClient`] on timeout.
pub fn wait_for_new_client(session: &SessionTarget, known: &[ClientId]) -> Result<ClientId, Error> {
    let deadline = Instant::now() + NEW_CLIENT_WITHIN;
    loop {
        match fetch_state(session) {
            Ok(state) => {
                if let Some(id) = pick_new(&state, known) {
                    return Ok(id);
                }
            }
            Err(err) => tracing::debug!(%err, "state poll failed"),
        }
        if Instant::now() >= deadline {
            return Err(Error::NoNewClient);
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
    fn known_ids_parse() {
        assert_eq!(parse_known(""), Ok(vec![]));
        assert_eq!(parse_known("1,2, 7"), Ok(vec![1, 2, 7]));
        assert_eq!(parse_known("1,x"), Err(Error::BadKnown("x".into())));
    }

    #[test]
    fn picks_the_lowest_new_client() {
        let s = state(&[(0, "a")], &[(1, 0), (5, 0), (3, 0)]);
        assert_eq!(pick_new(&s, &[1]), Some(3));
        assert_eq!(pick_new(&s, &[]), Some(1));
        assert_eq!(pick_new(&s, &[1, 3, 5]), None);
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
    fn attach_refuses_inside_a_session() {
        assert_eq!(
            check_outside_zellij(Some("w")),
            Err(Error::InsideSession("w".into()))
        );
        assert_eq!(check_outside_zellij(Some("")), Ok(()));
        assert_eq!(check_outside_zellij(None), Ok(()));
        assert_eq!(
            Error::InsideSession("w".into()).to_string(),
            "already inside zellij session \"w\" — use bind instead"
        );
    }

    #[test]
    fn default_session_name() {
        assert_eq!(attach_session(None), "warpify");
        assert_eq!(attach_session(Some("x")), "x");
    }

    #[test]
    fn not_moved_message() {
        assert_eq!(
            Error::NotMoved(2).to_string(),
            "bind sent but client 2 didn't move — is it connected?"
        );
    }
}

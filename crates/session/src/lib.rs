//! The plugin's session logic, free of zellij types so it builds and tests on the host: the
//! plugin converts what zellij reports into these snapshots and asks them what to answer.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use warpify_proto::{
    base_tab_name, Client, ClientId, Request, State, Tab, TabId, Target, CONFIG_FALSE,
    CONFIG_ON_CONNECT, CONFIG_PIN, CONFIG_TITLE, CONFIG_TITLE_PREFIX, CONFIG_TRUE,
    ON_CONNECT_NEW_TAB, ON_CONNECT_NONE,
};

/// What the plugin does for a client that has just connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnConnect {
    /// Nothing: the client stays wherever zellij put it.
    #[default]
    None,
    /// Give the client a tab of its own (a client that is alone in the session when it connects stays on its tab; any other gets a new tab).
    NewTab,
}

/// The plugin's `load_plugins` configuration (graph @nick/warpify, node #16).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub on_connect: OnConnect,
    /// Pin the bind made on connect.
    pub pin: bool,
    /// Put the prefix and the tab list into the title of the own client's focused pane (graph
    /// @nick/warpify, node #23). This writes over the pane's name: a name the user or a layout
    /// gave it is overwritten while the pane is focused and cleared after. Tabs with a default
    /// name, which zellij keeps in step with their single pane's name, get their names fixed
    /// (see [`Snapshot::plan_tab_names`]).
    pub title: bool,
    /// Opens the title; empty: the title is just the tab list.
    pub title_prefix: String,
}

impl Config {
    /// Parses the string map zellij hands to `load`. An unknown value falls back to the default
    /// and yields a warning; unknown keys are ignored.
    #[must_use]
    pub fn parse(map: &BTreeMap<String, String>) -> (Self, Vec<String>) {
        let mut config = Self::default();
        let mut warnings = Vec::new();
        let mut warn = |key: &str, value: &str, default: &str| {
            warnings.push(format!(
                "unknown value {value:?} for {key}, using {default:?}"
            ));
        };
        if let Some(value) = map.get(CONFIG_ON_CONNECT) {
            match value.as_str() {
                ON_CONNECT_NEW_TAB => config.on_connect = OnConnect::NewTab,
                ON_CONNECT_NONE => {}
                _ => warn(CONFIG_ON_CONNECT, value, ON_CONNECT_NONE),
            }
        }
        if let Some(value) = map.get(CONFIG_PIN) {
            match value.as_str() {
                CONFIG_TRUE => config.pin = true,
                CONFIG_FALSE => {}
                _ => warn(CONFIG_PIN, value, CONFIG_FALSE),
            }
        }
        if let Some(value) = map.get(CONFIG_TITLE) {
            match value.as_str() {
                CONFIG_TRUE => config.title = true,
                CONFIG_FALSE => {}
                _ => warn(CONFIG_TITLE, value, CONFIG_FALSE),
            }
        }
        if let Some(value) = map.get(CONFIG_TITLE_PREFIX) {
            value.trim().clone_into(&mut config.title_prefix);
        }
        (config, warnings)
    }
}

/// Longest title, in characters, the plugin composes.
pub const TITLE_MAX_CHARS: usize = 120;

/// Separator between the parts of the composed title.
const TITLE_SEP: &str = " · ";

/// Starts every composed title. zellij names a default-named tab with a single pane after the
/// pane's title, which is our rename, so a tab name can carry a composed title back to us; the
/// mark says so (invisible, and no tab name of its own starts with it).
const TITLE_MARK: char = '\u{200b}';

/// Follows the mark in the variants of a title: a leading `TITLE_MARK` followed by none, one or
/// two of these. zellij emits a window title only when the text differs from the last one it
/// emitted for the tab (`tiled_panes/mod.rs` `window_title`), and it remembers that per tab, so
/// a client arriving on a pane whose name is, or is set back to, that text gets no title; another
/// variant changes the text, not what is seen (graph @nick/warpify, node #23).
const TITLE_ALT: char = '\u{200c}';

/// How many variants a title has.
const TITLE_VARIANTS: usize = 3;

/// The variant and the body of a title of ours, `None` for any other text.
fn title_parts(text: &str) -> Option<(usize, &str)> {
    let rest = text.strip_prefix(TITLE_MARK)?;
    let body = rest.trim_start_matches(TITLE_ALT);
    Some(((rest.len() - body.len()) / TITLE_ALT.len_utf8(), body))
}

/// The body of a title of ours (any variant), `None` for any other text.
fn title_body(text: &str) -> Option<&str> {
    title_parts(text).map(|(_, body)| body)
}

/// The title with this body in this variant.
fn title_variant(body: &str, variant: usize) -> String {
    let alt: String = std::iter::repeat_n(TITLE_ALT, variant).collect();
    format!("{TITLE_MARK}{alt}{body}")
}

/// The tab's own name: without the exit suffix, and when zellij took a composed title for the
/// name, the bracketed part of it, which is the name of the tab the title was composed on. A
/// composed title that was cut leaves nothing to take: an ellipsis. The marks that
/// [`Snapshot::plan_tab_names`] may add behind a name go too. A name of ours is never returned
/// as it is (graph @nick/warpify, node #23).
#[must_use]
pub fn plain_tab_name(name: &str) -> &str {
    let base = base_tab_name(name);
    let Some(composed) = title_body(base) else {
        return base.trim_end_matches([TITLE_MARK, TITLE_ALT]);
    };
    bracketed(composed).unwrap_or("…")
}

/// The bracketed part of a composed title (without the mark).
fn bracketed(composed: &str) -> Option<&str> {
    composed
        .split(TITLE_SEP)
        .find_map(|part| part.strip_prefix('[')?.strip_suffix(']'))
}

/// The title for a client on the tab `own`: `<prefix> · <tab> · [<own tab>] · <tab>`, tabs in
/// position order, names without zellij's exit suffix, an empty prefix left out, the tab list
/// cut to `max_chars` with an ellipsis, then the mark. Empty when there are no tabs.
#[must_use]
pub fn compose_title(prefix: &str, tabs: &[TabSnapshot], own: TabId, max_chars: usize) -> String {
    if tabs.is_empty() {
        return String::new();
    }
    let mut ordered: Vec<&TabSnapshot> = tabs.iter().collect();
    ordered.sort_by_key(|t| t.position);
    let prefix = prefix.trim();
    let parts: Vec<String> = Some(prefix.to_owned())
        .filter(|p| !p.is_empty())
        .into_iter()
        .chain(ordered.iter().map(|t| {
            let name = plain_tab_name(&t.name);
            if t.id == own {
                format!("[{name}]")
            } else {
                name.to_owned()
            }
        }))
        .collect();
    let title = parts.join(TITLE_SEP);
    let room = max_chars.saturating_sub(1);
    let title: String = if title.chars().count() <= max_chars {
        title
    } else {
        title
            .chars()
            .take(room)
            .chain(std::iter::once('…'))
            .collect()
    };
    format!("{TITLE_MARK}{title}")
}

/// The text to name the pane when its actual name is `current`, the text this instance set last
/// is `last` and `wanted` (the first variant) is the title. A name that already is the title
/// stays, unless the client has just arrived (`arrived`): zellij may still hold that very text
/// as the last one it emitted for the tab, so a variant is taken that is neither the actual name
/// nor the last text set. Without arriving, the text set last is set again (the actual name
/// may not be reported yet), else the first variant.
fn pick_variant(wanted: &str, current: Option<&str>, last: Option<&str>, arrived: bool) -> String {
    let Some((_, body)) = title_parts(wanted) else {
        return wanted.to_owned();
    };
    let same = |text: Option<&str>| {
        text.and_then(title_parts)
            .and_then(|(variant, b)| (b == body).then_some(variant))
    };
    if !arrived {
        if let Some(variant) = same(current).or_else(|| same(last)) {
            return title_variant(body, variant);
        }
        return wanted.to_owned();
    }
    let taken = [same(current), same(last)];
    let free = (0..TITLE_VARIANTS)
        .find(|v| !taken.contains(&Some(*v)))
        .unwrap_or(0);
    title_variant(body, free)
}

/// The terminal pane a client has focused and the tab it is on, as zellij reports them together
/// (`get_focused_pane_info`), so a title never pairs a pane with another tab's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Focus {
    pub tab: TabId,
    pub pane: u32,
}

/// What `get_focused_pane_info` told about the own client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focused {
    /// A terminal pane.
    Terminal(Focus),
    /// Something that isn't a terminal pane (a plugin pane).
    Other,
    /// The call failed or timed out: the title stays as it is, nothing is planned.
    Unknown,
}

/// What the plugin does to a pane's name to keep the title in step (see
/// [`Snapshot::plan_title`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TitleOp {
    /// Name the terminal pane this text.
    Rename(u32, String),
    /// Give the pane's name back to the pane: an empty name makes zellij show the title the
    /// program set (zellij-server 0.45.1, `panes/terminal_pane.rs` `render_terminal_title`).
    Restore(u32),
}

/// What to do for a newly served client (see [`Snapshot::on_connect`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectAction {
    /// Bound to the tab it is on, already recorded in the snapshot: nothing to execute.
    BoundCurrent(TabId),
    /// Create a new tab for it: bind to `Target::New(None)` with this pin.
    NewTab { pin: bool },
}

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The client this instance belongs to.
    pub own_client: ClientId,
    pub tabs: Vec<TabSnapshot>,
    /// Set while this instance's client is bound (graph @nick/warpify, node #9).
    pub binding: Option<Binding>,
    /// A bind whose tab zellij hasn't reported yet (its action timed out): adopted by a later
    /// tab update once the own client is seen on the tab.
    pub pending: Option<PendingBind>,
    /// False once the instance is frozen: its client has left, zellij keeps the instance loaded
    /// and still pipes every message to it, but no tab update reaches it until a client takes
    /// the id again. A frozen instance stays silent (graph @nick/warpify, node #9, risk #12).
    pub connected: bool,
    /// A `SessionUpdate` reported zero connected clients and the `ListClients` reply that
    /// confirms or refutes it is still awaited (a disk-scan `SessionUpdate` can carry a stale
    /// zero). An empty `ListClients` is taken as "no clients", although zellij's list can miss a
    /// client whose focused pane isn't in the layout dump (e.g. in the scrollback editor):
    /// accepted residual risk (graph @nick/warpify, node #11).
    pub zero_pending: bool,
    /// Where `on_connect` stands for this client's lifetime; `freeze` resets it, so the next
    /// client to take the id is served again (graph @nick/warpify, node #16).
    pub connect: ConnectPhase,
    /// Terminal panes this instance titled, with the tab each was titled on: only a hint of
    /// which panes to look at for giving the name back; whether a pane is ours is read from its
    /// actual name in [`Self::pane_titles`]. Kept across a freeze (graph @nick/warpify,
    /// node #23).
    pub titled: BTreeMap<u32, TabId>,
    /// The actual name (the title when it has none) of every terminal pane, from the last
    /// `PaneUpdate`.
    pub pane_titles: BTreeMap<u32, String>,
    /// The terminal pane the own client had focused when the title was last planned, to tell a
    /// client arriving on a pane from one staying on it.
    pub last_focus: Option<u32>,
    /// The text this instance last set on each pane.
    pub last_text: BTreeMap<u32, String>,
}

/// The steps of serving a newly connected client. A tab update can't tell whether the client is
/// alone (zellij caches events per plugin instance regardless of client, so a view of another
/// client from before this one registered looks like "alone"), so the first update only asks
/// for the client list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectPhase {
    /// Nothing seen yet since the start or the last freeze.
    #[default]
    Idle,
    /// The client list was asked for (`connect_pending`); its reply decides.
    Pending,
    /// The reply said this many clients, but the own client's tab isn't known yet: decide on a
    /// later tab update with the same count.
    AwaitingTab(usize),
    /// Acted on.
    Served,
}

/// What a tab update means for serving the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectStep {
    Nothing,
    /// Call `list_clients()`; the reply goes to [`Snapshot::on_connect_clients`].
    AskClients,
    Act(ConnectAction),
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            own_client: ClientId::default(),
            tabs: Vec::new(),
            binding: None,
            pending: None,
            connected: true,
            zero_pending: false,
            connect: ConnectPhase::Idle,
            titled: BTreeMap::new(),
            pane_titles: BTreeMap::new(),
            last_focus: None,
            last_text: BTreeMap::new(),
        }
    }
}

/// What a bind that returned no tab id was after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingKind {
    /// The tab with this name, focused or created.
    FocusOrCreate(String),
    /// A new tab: the one the client lands on that wasn't there when the bind started.
    CreateNew,
}

/// A bind waiting for its tab to show up in a tab update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingBind {
    pub kind: PendingKind,
    pub pin: bool,
    /// Tabs known when the bind started, so an old active tab isn't taken for the new one.
    known: Vec<TabId>,
}

impl PendingBind {
    #[must_use]
    pub fn new(kind: PendingKind, pin: bool, tabs: &[TabSnapshot]) -> Self {
        Self {
            kind,
            pin,
            known: tabs.iter().map(|t| t.id).collect(),
        }
    }

    /// The tab this bind ended up on, if the own client is on it in `tabs`.
    #[must_use]
    pub fn resolve(&self, tabs: &[TabSnapshot]) -> Option<TabId> {
        let active = tabs.iter().find(|t| t.active)?;
        match &self.kind {
            PendingKind::FocusOrCreate(name) => {
                (plain_tab_name(&active.name) == plain_tab_name(name)).then_some(active.id)
            }
            PendingKind::CreateNew => (!self.known.contains(&active.id)).then_some(active.id),
        }
    }
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
        Target::Name(name) => tabs
            .iter()
            .find(|t| plain_tab_name(&t.name) == plain_tab_name(name))
            .map_or_else(
                || BindStep::FocusOrCreate(name.clone()),
                |t| BindStep::GoTo(t.id),
            ),
        Target::New(name) => BindStep::CreateNew(name.clone()),
    }
}

/// What a bound instance must do after a tab update; `None` when nothing is wrong.
#[must_use]
pub fn on_tabs(binding: Option<Binding>, snapshot: &Snapshot) -> Option<Correction> {
    if !snapshot.connected {
        return None;
    }
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
    /// `client` has left the session: the instance that belongs to it freezes, since zellij
    /// keeps that instance and hands it to the next client with the same id (graph
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
                name: plain_tab_name(&t.name).to_owned(),
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

    /// Apply `Internal::Forget`: only the instance of that client freezes.
    pub fn forget(&mut self, client: ClientId) {
        if client == self.own_client {
            self.freeze();
        }
    }

    /// The client is gone: drop the binding and go silent until a tab update says a client holds
    /// this id again.
    pub fn freeze(&mut self) {
        self.connected = false;
        self.zero_pending = false;
        self.connect = ConnectPhase::Idle;
        self.binding = None;
        self.pending = None;
    }

    /// A tab update arrived, so a client holds this id: un-freeze (a fresh start, the binding
    /// stays cleared) and adopt a pending bind whose tab the client is now on.
    pub fn apply_tabs(&mut self, tabs: Vec<TabSnapshot>) {
        self.tabs = tabs;
        self.connected = true;
        // A tab update proves our own client is connected: a later empty list can't freeze.
        self.zero_pending = false;
        if let Some(tab) = self.pending.as_ref().and_then(|p| p.resolve(&self.tabs)) {
            let pin = self.pending.take().is_some_and(|p| p.pin);
            self.binding = Some(Binding::new(tab, pin));
        }
    }

    /// Take the actual names of the terminal panes from a `PaneUpdate`.
    pub fn apply_panes(&mut self, titles: BTreeMap<u32, String>) {
        self.pane_titles = titles;
    }

    /// The pane renames that bring the title in step, given where the own client's focus is now.
    /// Nothing while the option is off, the instance is frozen or the focus is unknown. The
    /// focused pane is renamed when its actual name isn't the title; when the name already is
    /// the title but the client has just arrived on the pane, the other variant of the title is
    /// set, since zellij emits a window title only when its text changed ([`TITLE_ALT`]), and
    /// the same goes for a pane given back and named again on arrival. A pane
    /// this instance titled is given back when the own client isn't on it, no other client is on
    /// its tab and its actual name still carries our mark. Memory (`titled`) only says where to
    /// look (graph @nick/warpify, node #23).
    pub fn plan_title(&mut self, config: &Config, focus: Focused) -> Vec<TitleOp> {
        if !config.title || !self.connected {
            return Vec::new();
        }
        let focus = match focus {
            Focused::Unknown => return Vec::new(),
            Focused::Other => None,
            // A tab this snapshot doesn't list yet: the tab update that lists it plans again,
            // and a title composed now would carry no bracketed tab name.
            Focused::Terminal(f) if self.tabs.iter().all(|t| t.id != f.tab) => return Vec::new(),
            Focused::Terminal(focus) => Some(focus),
        };
        let mut ops = self.plan_restores(focus.map(|f| f.pane));
        let arrived = self.last_focus != focus.map(|f| f.pane);
        self.last_focus = focus.map(|f| f.pane);
        if let Some(Focus { tab, pane }) = focus {
            let wanted = compose_title(&config.title_prefix, &self.tabs, tab, TITLE_MAX_CHARS);
            if !wanted.is_empty() {
                let current = self.pane_titles.get(&pane).map(String::as_str);
                let last = self.last_text.get(&pane).map(String::as_str);
                let text = pick_variant(&wanted, current, last, arrived);
                self.titled.insert(pane, tab);
                if current != Some(text.as_str()) {
                    self.last_text.insert(pane, text.clone());
                    ops.push(TitleOp::Rename(pane, text));
                }
            }
        }
        ops
    }

    /// The panes to give back (see [`Self::plan_title`]); forgets the ones that are gone.
    fn plan_restores(&mut self, own_pane: Option<u32>) -> Vec<TitleOp> {
        let mut ops = Vec::new();
        for (pane, tab) in self.titled.clone() {
            if Some(pane) == own_pane {
                continue;
            }
            let tab_there = self.tabs.iter().find(|t| t.id == tab);
            let gone = !self.pane_titles.is_empty() && !self.pane_titles.contains_key(&pane);
            if tab_there.is_none() || gone {
                self.titled.remove(&pane);
                self.last_text.remove(&pane);
                continue;
            }
            let shared = tab_there.is_some_and(|t| !t.other_clients.is_empty());
            let ours = self
                .pane_titles
                .get(&pane)
                .is_some_and(|t| title_body(t).is_some());
            if ours && !shared {
                self.titled.remove(&pane);
                ops.push(TitleOp::Restore(pane));
            }
        }
        ops
    }

    /// The tab renames the leader makes while the title is on: zellij names a default-named tab
    /// with a single pane after the pane's title, so once a pane of such a tab is titled the tab
    /// is named with our title (`TabInfo` doesn't say whether a name is the default; a name
    /// carrying our mark can only be that tracking, since a tab with any other name shows
    /// its own). Each gets the name it had: the bracketed part of the title. With nothing to
    /// take (a cut title) the default name zellij would give it, and a name that is the default
    /// gets a trailing mark, or zellij would go on taking the pane's title for it (zellij-server
    /// 0.45.1, `tab/mod.rs` `tab_name_is_default`). The tab reports the new name next, so the
    /// rename isn't repeated (graph @nick/warpify, node #23).
    #[must_use]
    pub fn plan_tab_names(&self, config: &Config) -> Vec<(TabId, String)> {
        if !config.title || !self.is_leader() {
            return Vec::new();
        }
        self.tabs
            .iter()
            .filter_map(|t| {
                let composed = title_body(base_tab_name(&t.name))?;
                let default = format!("Tab #{}", t.id + 1);
                let name = bracketed(composed).map_or(default.clone(), str::to_owned);
                let name = if name == default {
                    format!("{name}{TITLE_MARK}")
                } else {
                    name
                };
                Some((t.id, name))
            })
            .collect()
    }

    /// Serve a newly connected client, once per client lifetime: call after [`Self::apply_tabs`],
    /// which the first tab update after `load` and the one that un-freezes the instance both
    /// are. With `on_connect = new_tab` the first such update acts on nothing: it only
    /// asks for the client list ([`ConnectStep::AskClients`]), since a tab update can't say
    /// whether this client is alone (zellij replays cached events of other clients to a fresh
    /// instance; observed live in zellij 0.45.1; graph @nick/warpify, node #16). The reply is
    /// [`Self::on_connect_clients`]'s; if it arrived before the own tab was known, the next
    /// update decides here.
    pub fn on_connect(&mut self, config: &Config) -> ConnectStep {
        if config.on_connect == OnConnect::None {
            return ConnectStep::Nothing;
        }
        match self.connect {
            ConnectPhase::Idle => {
                self.connect = ConnectPhase::Pending;
                ConnectStep::AskClients
            }
            ConnectPhase::AwaitingTab(count) => self
                .decide_connect(config.pin, count)
                .map_or(ConnectStep::Nothing, ConnectStep::Act),
            ConnectPhase::Pending | ConnectPhase::Served => ConnectStep::Nothing,
        }
    }

    /// The `ListClients` reply as an answer to [`Self::on_connect`]'s question: more than one
    /// client means a new tab; otherwise (an empty list is the own client missing from an
    /// incomplete list) bind the tab the own client is on, waiting for the next update if it
    /// isn't known yet. A reply nobody asked for does nothing.
    pub fn on_connect_clients(
        &mut self,
        config: &Config,
        client_count: usize,
    ) -> Option<ConnectAction> {
        if self.connect != ConnectPhase::Pending || config.on_connect == OnConnect::None {
            return None;
        }
        self.decide_connect(config.pin, client_count).or_else(|| {
            self.connect = ConnectPhase::AwaitingTab(client_count);
            None
        })
    }

    /// Act on the client count, if the answer is available now: marks the client served.
    fn decide_connect(&mut self, pin: bool, client_count: usize) -> Option<ConnectAction> {
        if client_count > 1 {
            self.connect = ConnectPhase::Served;
            return Some(ConnectAction::NewTab { pin });
        }
        let tab = self.tabs.iter().find(|t| t.active)?.id;
        self.connect = ConnectPhase::Served;
        self.binding = Some(Binding::new(tab, pin).seen_in(&self.tabs));
        Some(ConnectAction::BoundCurrent(tab))
    }

    /// A `SessionUpdate` reported zero connected clients: whether to ask `list_clients` to
    /// confirm. A frozen instance has nothing left to freeze, so it doesn't ask.
    pub fn on_zero_clients(&mut self) -> bool {
        self.zero_pending = self.connected;
        self.zero_pending
    }

    /// The `ListClients` reply arrived: freezes the instance and returns true only if a zero is
    /// pending and the list is empty. A non-empty list refutes the zero; either way the pending
    /// flag is cleared, and a reply nobody asked for does nothing.
    pub fn on_client_list(&mut self, client_count: usize) -> bool {
        if !std::mem::take(&mut self.zero_pending) {
            return false;
        }
        let freeze = client_count == 0 && self.connected;
        if freeze {
            self.freeze();
        }
        freeze
    }

    /// Record the outcome of a bind step: the tab zellij returned is the binding, no tab (the
    /// action timed out) leaves a pending bind for a later tab update. `Fail` holds nothing.
    pub fn finish_bind(&mut self, step: &BindStep, pin: bool, tab: Option<TabId>) {
        self.binding = None;
        self.pending = None;
        if let Some(tab) = tab {
            // zellij sends no tab update for a switch to the tab the client is already on.
            self.binding = Some(Binding::new(tab, pin).seen_in(&self.tabs));
            return;
        }
        let kind = match step {
            BindStep::FocusOrCreate(name) => PendingKind::FocusOrCreate(name.clone()),
            BindStep::CreateNew(_) => PendingKind::CreateNew,
            BindStep::GoTo(_) | BindStep::Fail(_) => return,
        };
        self.pending = Some(PendingBind::new(kind, pin, &self.tabs));
    }

    /// The instance of the lowest connected client speaks for the session. With no client known
    /// yet every instance does: a duplicate reply beats a CLI left hanging.
    #[must_use]
    pub fn is_leader(&self) -> bool {
        self.connected
            && self
                .clients()
                .iter()
                .map(|c| c.id)
                .min()
                .is_none_or(|leader| leader == self.own_client)
    }

    /// Whether this instance handles `request` (`None`: it didn't parse). A bind is the named
    /// client's own instance's job; anything else is answered once, by the leader, or the CLI
    /// would get a copy per client. A frozen instance handles nothing.
    #[must_use]
    pub fn handles(&self, request: Option<&Request>) -> bool {
        if !self.connected {
            return false;
        }
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
            ..Snapshot::default()
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
            ..Snapshot::default()
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
                ..Snapshot::default()
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
                ..Snapshot::default()
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
            ..Snapshot::default()
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
            ..Snapshot::default()
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
    fn forget_freezes_only_the_named_clients_instance() {
        let (_, mut snap) = bound(7, true, vec![tab(7, 0, true, &[])]);
        snap.forget(2);
        assert!(snap.binding.is_some());
        assert!(snap.connected);
        snap.forget(1);
        assert_eq!(snap.binding, None);
        assert!(!snap.connected);
    }

    #[test]
    fn frozen_instance_handles_nothing_and_never_leads() {
        let mut snap = with_clients(1, &[]);
        snap.freeze();
        let bind = Request::Bind {
            client: 1,
            target: Target::Id(0),
            pin: false,
        };
        for request in [
            Some(&Request::State),
            Some(&Request::Watch),
            Some(&bind),
            None,
        ] {
            assert!(!snap.handles(request));
        }
        assert!(!snap.is_leader());
    }

    #[test]
    fn frozen_instance_makes_no_corrections() {
        let (b, mut snap) = bound(9, true, vec![named(3, "a", true), named(9, "b", false)]);
        snap.freeze();
        assert_eq!(on_tabs(b, &snap), None);
    }

    #[test]
    fn tab_update_unfreezes_without_a_binding() {
        let (_, mut snap) = bound(9, true, vec![named(9, "b", true)]);
        snap.freeze();
        snap.apply_tabs(vec![named(3, "a", true), named(9, "b", false)]);
        assert!(snap.connected);
        assert_eq!(snap.binding, None);
        assert!(snap.is_leader());
        assert_eq!(on_tabs(snap.binding, &snap), None);
    }

    #[test]
    fn leader_choice_skips_a_frozen_self() {
        // Client 1 is the lowest id, yet frozen: it must not claim the leadership.
        let mut frozen = with_clients(1, &[2]);
        frozen.freeze();
        assert!(!frozen.is_leader());
        assert!(with_clients(2, &[]).is_leader());
    }

    #[test]
    fn timed_out_focus_or_create_is_adopted_on_the_named_tab() {
        let mut snap = with_clients(1, &[]);
        snap.tabs = vec![named(3, "a", true)];
        snap.finish_bind(&BindStep::FocusOrCreate("z".into()), true, None);
        assert_eq!(snap.binding, None);
        // Still on "a": not yet.
        snap.apply_tabs(vec![named(3, "a", true)]);
        assert_eq!(snap.binding, None);
        // Another tab named "z" that the client isn't on: not yet.
        snap.apply_tabs(vec![named(3, "a", true), named(5, "z", false)]);
        assert_eq!(snap.binding, None);
        snap.apply_tabs(vec![named(3, "a", false), named(5, "z", true)]);
        assert_eq!(snap.binding, Some(Binding::new(5, true)));
        assert_eq!(snap.pending, None);
    }

    #[test]
    fn timed_out_create_new_is_adopted_on_the_new_active_tab() {
        let mut snap = with_clients(1, &[]);
        snap.tabs = vec![named(3, "a", true)];
        snap.finish_bind(&BindStep::CreateNew(None), false, None);
        // The old active tab is not the new one.
        snap.apply_tabs(vec![named(3, "a", true)]);
        assert_eq!(snap.binding, None);
        snap.apply_tabs(vec![named(3, "a", false), named(8, "Tab #2", true)]);
        assert_eq!(snap.binding, Some(Binding::new(8, false)));
    }

    #[test]
    fn freezing_or_binding_again_drops_a_pending_bind() {
        let mut snap = with_clients(1, &[]);
        snap.finish_bind(&BindStep::CreateNew(None), false, None);
        assert!(snap.pending.is_some());
        snap.finish_bind(&BindStep::GoTo(3), true, Some(3));
        assert_eq!(snap.pending, None);
        assert_eq!(snap.binding, Some(Binding::new(3, true)));
        snap.finish_bind(&BindStep::CreateNew(None), false, None);
        snap.freeze();
        assert_eq!(snap.pending, None);
    }

    #[test]
    fn bind_to_the_current_tab_is_in_force_at_once() {
        let mut snap = with_clients(1, &[]);
        snap.tabs = vec![named(3, "a", true), named(5, "b", false)];
        snap.finish_bind(&BindStep::GoTo(3), true, Some(3));
        assert_eq!(snap.binding.map(|b| b.seen), Some(true));
        // The next update, with the client drifted away, is pulled back.
        snap.apply_tabs(vec![named(3, "a", false), named(5, "b", true)]);
        assert_eq!(on_tabs(snap.binding, &snap), Some(Correction::GoTo(3)));
    }

    #[test]
    fn bind_to_another_tab_waits_to_be_seen() {
        let mut snap = with_clients(1, &[]);
        snap.tabs = vec![named(3, "a", true), named(5, "b", false)];
        snap.finish_bind(&BindStep::GoTo(5), true, Some(5));
        assert_eq!(snap.binding.map(|b| b.seen), Some(false));
    }

    #[test]
    fn zero_clients_sets_pending_and_asks() {
        let mut snap = with_clients(1, &[]);
        assert!(snap.on_zero_clients());
        assert!(snap.zero_pending);
        assert!(snap.connected, "not frozen until the list confirms");
    }

    #[test]
    fn pending_zero_with_empty_list_freezes() {
        let mut snap = with_clients(1, &[]);
        snap.on_zero_clients();
        assert!(snap.on_client_list(0));
        assert!(!snap.connected);
        assert!(!snap.zero_pending);
    }

    #[test]
    fn pending_zero_with_non_empty_list_is_ignored_and_cleared() {
        let mut snap = with_clients(1, &[]);
        snap.on_zero_clients();
        assert!(!snap.on_client_list(2));
        assert!(snap.connected);
        assert!(!snap.zero_pending);
    }

    #[test]
    fn tab_update_clears_pending_zero_so_empty_list_does_not_freeze() {
        let mut snap = with_clients(1, &[]);
        snap.on_zero_clients();
        assert!(snap.zero_pending);
        snap.apply_tabs(vec![named(3, "a", true)]);
        assert!(!snap.zero_pending);
        assert!(!snap.on_client_list(0));
        assert!(snap.connected);
    }

    #[test]
    fn client_list_without_pending_does_nothing() {
        let mut snap = with_clients(1, &[]);
        assert!(!snap.on_client_list(0));
        assert!(snap.connected);
    }

    #[test]
    fn frozen_instance_ignores_zero() {
        let mut snap = with_clients(1, &[]);
        snap.freeze();
        assert!(!snap.on_zero_clients());
        assert!(!snap.zero_pending);
        assert!(!snap.on_client_list(0));
    }

    #[test]
    fn exited_suffix_is_ignored_in_name_matching() {
        let tabs = [named(3, "logs [ EXITED ] ", true)];
        assert_eq!(
            plan_bind(&Target::Name("logs".into()), &tabs),
            BindStep::GoTo(3)
        );
        let pending = PendingBind::new(PendingKind::FocusOrCreate("logs".into()), false, &[]);
        assert_eq!(pending.resolve(&tabs), Some(3));
    }

    #[test]
    fn internal_message_round_trips() {
        let msg = Internal::Forget { client: 5 };
        assert_eq!(msg.encode(), r#"{"msg":"forget","client":5}"#);
        assert_eq!(Internal::decode(&msg.encode()).unwrap(), msg);
        assert!(Internal::decode("{}").is_err());
    }

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn config_defaults_to_nothing() {
        assert_eq!(Config::parse(&BTreeMap::new()), (Config::default(), vec![]));
        assert_eq!(Config::default().on_connect, OnConnect::None);
        assert!(!Config::default().pin);
    }

    #[test]
    fn config_reads_both_keys() {
        let (config, warnings) = Config::parse(&map(&[("on_connect", "new_tab"), ("pin", "true")]));
        assert_eq!(
            config,
            Config {
                on_connect: OnConnect::NewTab,
                pin: true,
                ..Config::default()
            }
        );
        assert!(warnings.is_empty());
        let (config, warnings) = Config::parse(&map(&[("on_connect", "none"), ("pin", "false")]));
        assert_eq!(config, Config::default());
        assert!(warnings.is_empty());
    }

    #[test]
    fn config_unknown_values_warn_and_default_and_unknown_keys_are_ignored() {
        let (config, warnings) = Config::parse(&map(&[
            ("on_connect", "new-tab"),
            ("pin", "yes"),
            ("other", "x"),
        ]));
        assert_eq!(config, Config::default());
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("on_connect") && warnings[0].contains("new-tab"));
        assert!(warnings[1].contains("pin"));
    }

    const NEW_TAB: Config = Config {
        on_connect: OnConnect::NewTab,
        pin: false,
        title: false,
        title_prefix: String::new(),
    };

    #[test]
    fn first_update_only_asks_for_the_client_list() {
        // Whatever the update shows, alone or not: no action on it.
        for mut snap in [with_clients(1, &[]), with_clients(2, &[1])] {
            assert_eq!(snap.on_connect(&NEW_TAB), ConnectStep::AskClients);
            assert_eq!(snap.connect, ConnectPhase::Pending);
            assert_eq!(snap.binding, None);
        }
    }

    #[test]
    fn several_clients_in_the_list_ask_for_a_new_tab() {
        let mut snap = with_clients(1, &[]);
        let config = Config {
            pin: true,
            ..NEW_TAB
        };
        snap.on_connect(&config);
        assert_eq!(
            snap.on_connect_clients(&config, 2),
            Some(ConnectAction::NewTab { pin: true })
        );
        assert_eq!(snap.connect, ConnectPhase::Served);
        assert_eq!(snap.binding, None);
    }

    #[test]
    fn a_lone_client_binds_the_tab_it_is_on() {
        // An empty list is the own client missing from an incomplete list: same as one.
        for count in [1, 0] {
            let mut snap = with_clients(1, &[]);
            let config = Config {
                pin: true,
                ..NEW_TAB
            };
            snap.on_connect(&config);
            assert_eq!(
                snap.on_connect_clients(&config, count),
                Some(ConnectAction::BoundCurrent(0))
            );
            assert_eq!(
                snap.binding,
                Some(Binding {
                    tab: 0,
                    pin: true,
                    seen: true
                })
            );
            assert_eq!(snap.connect, ConnectPhase::Served);
        }
    }

    #[test]
    fn a_stale_view_of_another_client_doesnt_make_it_alone() {
        // The cached update shows no other clients, but the list has two.
        let mut snap = with_clients(2, &[]);
        assert_eq!(snap.on_connect(&NEW_TAB), ConnectStep::AskClients);
        assert_eq!(
            snap.on_connect_clients(&NEW_TAB, 2),
            Some(ConnectAction::NewTab { pin: false })
        );
    }

    #[test]
    fn later_updates_do_nothing() {
        let mut snap = with_clients(1, &[]);
        snap.on_connect(&NEW_TAB);
        assert_eq!(snap.on_connect(&NEW_TAB), ConnectStep::Nothing);
        snap.on_connect_clients(&NEW_TAB, 1);
        assert_eq!(snap.on_connect(&NEW_TAB), ConnectStep::Nothing);
        assert_eq!(snap.on_connect_clients(&NEW_TAB, 2), None);
    }

    #[test]
    fn unfreezing_asks_again() {
        let mut snap = with_clients(2, &[1]);
        snap.on_connect(&NEW_TAB);
        snap.on_connect_clients(&NEW_TAB, 2);
        snap.freeze();
        snap.apply_tabs(vec![tab(0, 0, true, &[])]);
        assert_eq!(snap.on_connect(&NEW_TAB), ConnectStep::AskClients);
    }

    #[test]
    fn on_connect_none_does_nothing() {
        let mut snap = with_clients(1, &[]);
        assert_eq!(snap.on_connect(&Config::default()), ConnectStep::Nothing);
        assert_eq!(snap.on_connect_clients(&Config::default(), 1), None);
        assert_eq!(snap.connect, ConnectPhase::Idle);
        assert_eq!(snap.binding, None);
    }

    #[test]
    fn a_reply_without_a_question_does_nothing() {
        let mut snap = with_clients(1, &[]);
        assert_eq!(snap.on_connect_clients(&NEW_TAB, 1), None);
        assert_eq!(snap.connect, ConnectPhase::Idle);
        assert_eq!(snap.binding, None);
    }

    #[test]
    fn a_reply_before_the_own_tab_is_known_waits_for_the_next_update() {
        let mut snap = Snapshot {
            own_client: 1,
            tabs: vec![tab(0, 0, false, &[2])],
            ..Snapshot::default()
        };
        assert_eq!(snap.on_connect(&NEW_TAB), ConnectStep::AskClients);
        assert_eq!(snap.on_connect_clients(&NEW_TAB, 1), None);
        assert_eq!(snap.connect, ConnectPhase::AwaitingTab(1));
        assert_eq!(snap.on_connect(&NEW_TAB), ConnectStep::Nothing);
        snap.apply_tabs(vec![tab(0, 0, true, &[])]);
        assert_eq!(
            snap.on_connect(&NEW_TAB),
            ConnectStep::Act(ConnectAction::BoundCurrent(0))
        );
        assert_eq!(snap.connect, ConnectPhase::Served);
    }

    #[test]
    fn one_reply_answers_both_the_zero_check_and_the_connect_question() {
        let mut snap = with_clients(1, &[]);
        snap.on_connect(&NEW_TAB);
        assert!(snap.on_zero_clients());
        // A non-empty list refutes the zero and serves the client.
        assert!(!snap.on_client_list(1));
        assert_eq!(
            snap.on_connect_clients(&NEW_TAB, 1),
            Some(ConnectAction::BoundCurrent(0))
        );
    }

    fn tab_named(id: TabId, position: usize, name: &str, active: bool) -> TabSnapshot {
        TabSnapshot {
            name: name.to_owned(),
            ..tab(id, position, active, &[])
        }
    }

    #[test]
    fn title_config_parses_and_warns() {
        let (config, warnings) = Config::parse(&map(&[
            ("terminal_title", "true"),
            ("title_prefix", " 🟠 x "),
        ]));
        assert!(config.title && warnings.is_empty());
        assert_eq!(config.title_prefix, "🟠 x");
        let (config, warnings) = Config::parse(&map(&[("terminal_title", "yes")]));
        assert!(!config.title);
        assert_eq!(warnings.len(), 1);
        assert!(!Config::parse(&BTreeMap::new()).0.title);
    }

    #[test]
    fn title_lists_tabs_in_order_with_the_own_in_brackets() {
        let tabs = [
            tab_named(2, 1, "logs [ EXITED ] ", true),
            tab_named(1, 0, "main", false),
            tab_named(3, 2, "two", false),
        ];
        assert_eq!(
            compose_title("🟠 test", &tabs, 2, 120),
            "\u{200b}🟠 test · main · [logs] · two"
        );
        assert_eq!(
            compose_title("", &tabs, 2, 120),
            "\u{200b}main · [logs] · two"
        );
        assert_eq!(compose_title("p", &[], 1, 120), "");
    }

    #[test]
    fn title_is_cut_with_an_ellipsis() {
        let tabs = [tab_named(1, 0, "abcdefghij", true)];
        let title = compose_title("pre", &tabs, 1, 10);
        assert_eq!(title.chars().count(), 11);
        assert_eq!(title, "\u{200b}pre · [ab…");
        assert_eq!(
            compose_title("pre", &tabs, 1, 120),
            "\u{200b}pre · [abcdefghij]"
        );
    }

    #[test]
    fn a_composed_title_taken_for_a_tab_name_does_not_grow() {
        // zellij names a single-pane default-named tab after the pane's title: ours.
        let mut tabs = [tab_named(1, 0, "Tab #1", true), tab_named(2, 1, "b", false)];
        let first = compose_title("P", &tabs, 1, 120);
        assert_eq!(first, "\u{200b}P · [Tab #1] · b");
        tabs[0].name = first.clone();
        assert_eq!(compose_title("P", &tabs, 1, 120), first);
        // another client's title, on the tab `b`, gives that tab's name back as well
        tabs[1].name = "\u{200b}P · a · [b] [ EXITED ] ".into();
        assert_eq!(compose_title("P", &tabs, 1, 120), first);
        // a cut title has no bracketed part left
        tabs[1].name = "\u{200b}P · a · [b…".into();
        assert_eq!(
            compose_title("P", &tabs, 1, 120),
            "\u{200b}P · [Tab #1] · …"
        );
    }

    fn titled_config() -> Config {
        Config {
            title: true,
            title_prefix: "P".into(),
            ..Config::default()
        }
    }

    fn title_snapshot() -> Snapshot {
        Snapshot {
            own_client: 1,
            tabs: vec![tab_named(1, 0, "a", true), tab_named(2, 1, "b", false)],
            ..Snapshot::default()
        }
    }

    fn at(tab: TabId, pane: u32) -> Focused {
        Focused::Terminal(Focus { tab, pane })
    }

    fn names(snap: &mut Snapshot, panes: &[(u32, &str)]) {
        snap.apply_panes(panes.iter().map(|(p, t)| (*p, (*t).to_owned())).collect());
    }

    const A1: &str = "\u{200b}P · [a] · b";
    const A2: &str = "\u{200b}\u{200c}P · [a] · b";
    const B1: &str = "\u{200b}P · a · [b]";

    #[test]
    fn title_renames_a_pane_whose_actual_name_differs() {
        let mut snap = title_snapshot();
        let config = titled_config();
        names(&mut snap, &[(4, "zsh")]);
        assert_eq!(
            snap.plan_title(&config, at(1, 4)),
            vec![TitleOp::Rename(4, A1.into())]
        );
        // zellij hasn't reported the new name yet: the same rename again, which changes nothing
        assert_eq!(
            snap.plan_title(&config, at(1, 4)),
            vec![TitleOp::Rename(4, A1.into())]
        );
        // the actual name is what we want: nothing, however often asked
        names(&mut snap, &[(4, A1)]);
        assert!(snap.plan_title(&config, at(1, 4)).is_empty());
        assert!(snap.plan_title(&config, at(1, 4)).is_empty());
        // the user renamed the pane meanwhile: ours again
        names(&mut snap, &[(4, "mine")]);
        assert_eq!(
            snap.plan_title(&config, at(1, 4)),
            vec![TitleOp::Rename(4, A1.into())]
        );
    }

    #[test]
    fn a_title_follows_the_client_to_another_tab() {
        let mut snap = title_snapshot();
        let config = titled_config();
        names(&mut snap, &[(4, A1)]);
        snap.plan_title(&config, at(1, 4));
        assert_eq!(
            snap.plan_title(&config, at(2, 4)),
            vec![TitleOp::Rename(4, B1.into())]
        );
    }

    #[test]
    fn a_client_arriving_on_a_pane_with_its_text_flips_the_variant() {
        let mut snap = title_snapshot();
        let config = titled_config();
        names(&mut snap, &[(4, A1)]);
        // arrives: the name is already the text, zellij would stay silent
        assert_eq!(
            snap.plan_title(&config, at(1, 4)),
            vec![TitleOp::Rename(4, A2.into())]
        );
        // staying is not arriving, and the variants are the same text to us
        names(&mut snap, &[(4, A2)]);
        assert!(snap.plan_title(&config, at(1, 4)).is_empty());
        // arriving again flips back
        snap.plan_title(&config, Focused::Other);
        assert_eq!(
            snap.plan_title(&config, at(1, 4)),
            vec![TitleOp::Rename(4, A1.into())]
        );
        // a name that isn't ours or has another body gets the first variant
        names(&mut snap, &[(4, "zsh")]);
        snap.plan_title(&config, Focused::Other);
        assert_eq!(
            snap.plan_title(&config, at(1, 4)),
            vec![TitleOp::Rename(4, A2.into())]
        );
        names(&mut snap, &[(4, A2)]);
        assert_eq!(
            snap.plan_title(&config, at(2, 4)),
            vec![TitleOp::Rename(4, B1.into())]
        );
    }

    #[test]
    fn a_client_coming_back_to_a_pane_given_back_gets_another_variant() {
        // the pane's tab was left with no client, so zellij still holds the text last emitted
        let mut snap = title_snapshot();
        let config = titled_config();
        names(&mut snap, &[(4, "zsh"), (5, "zsh")]);
        snap.plan_title(&config, at(1, 4));
        names(&mut snap, &[(4, A1), (5, "zsh")]);
        snap.plan_title(&config, at(2, 5));
        names(&mut snap, &[(4, ""), (5, B1)]);
        assert_eq!(
            snap.plan_title(&config, at(1, 4)),
            vec![TitleOp::Restore(5), TitleOp::Rename(4, A2.into())]
        );
    }

    #[test]
    fn variants_both_count_as_ours() {
        assert_eq!(title_body("\u{200b}x"), Some("x"));
        assert_eq!(title_body("\u{200b}\u{200c}x"), Some("x"));
        assert_eq!(title_body("x"), None);
        assert_eq!(title_body("\u{200c}x"), None);
        assert_eq!(plain_tab_name("\u{200b}\u{200c}P · [a] · b"), "a");
        assert_eq!(plain_tab_name("\u{200b}P · [a] · b"), "a");
        let (v0, v1, v2) = (
            "\u{200b}x",
            "\u{200b}\u{200c}x",
            "\u{200b}\u{200c}\u{200c}x",
        );
        assert_eq!(title_parts(v2), Some((2, "x")));
        assert_eq!(pick_variant(v0, None, None, true), v0);
        assert_eq!(pick_variant(v0, Some(v0), None, true), v1);
        assert_eq!(pick_variant(v0, Some(v1), None, true), v0);
        // neither the actual name nor the text set last
        assert_eq!(pick_variant(v0, Some(v0), Some(v1), true), v2);
        assert_eq!(pick_variant(v0, Some(""), Some(v0), true), v1);
        assert_eq!(pick_variant(v0, Some("zsh"), Some("\u{200b}y"), true), v0);
        // not arriving: what is there stays, else what was set last, else the first
        assert_eq!(pick_variant(v0, Some(v1), None, false), v1);
        assert_eq!(pick_variant(v0, Some("zsh"), Some(v1), false), v1);
        assert_eq!(pick_variant(v0, Some("zsh"), None, false), v0);
    }

    #[test]
    fn title_gives_the_pane_back_when_focus_leaves_and_nobody_else_is_there() {
        let mut snap = title_snapshot();
        let config = titled_config();
        names(&mut snap, &[(4, "zsh"), (5, "zsh")]);
        snap.plan_title(&config, at(1, 4));
        names(&mut snap, &[(4, A1), (5, "zsh")]);
        assert_eq!(
            snap.plan_title(&config, at(1, 5)),
            vec![TitleOp::Restore(4), TitleOp::Rename(5, A1.into())]
        );
        names(&mut snap, &[(4, ""), (5, A1)]);
        assert_eq!(
            snap.plan_title(&config, Focused::Other),
            vec![TitleOp::Restore(5)]
        );
        assert!(snap.titled.is_empty());
    }

    #[test]
    fn title_is_kept_for_a_client_still_on_the_tab() {
        let mut snap = title_snapshot();
        let config = titled_config();
        names(&mut snap, &[(4, A1), (5, "zsh")]);
        snap.plan_title(&config, at(1, 4));
        // another client came to tab 1
        snap.tabs[0].other_clients = vec![2];
        assert_eq!(
            snap.plan_title(&config, at(2, 5)),
            vec![TitleOp::Rename(5, B1.into())]
        );
        assert!(snap.titled.contains_key(&4));
        // it leaves: the pane is given back on the next look
        snap.tabs[0].other_clients = Vec::new();
        names(&mut snap, &[(4, A1), (5, B1)]);
        assert_eq!(
            snap.plan_title(&config, at(2, 5)),
            vec![TitleOp::Restore(4)]
        );
    }

    #[test]
    fn a_pane_not_carrying_our_mark_is_left_alone() {
        let mut snap = title_snapshot();
        let config = titled_config();
        names(&mut snap, &[(4, A1), (5, "zsh")]);
        snap.plan_title(&config, at(1, 4));
        // the user gave the pane a name of their own, or it was already restored
        names(&mut snap, &[(4, "mine"), (5, "zsh")]);
        assert_eq!(
            snap.plan_title(&config, at(1, 5)),
            vec![TitleOp::Rename(5, A1.into())]
        );
    }

    #[test]
    fn a_focus_on_a_tab_not_listed_yet_waits_for_the_tab_update() {
        let mut snap = title_snapshot();
        let config = titled_config();
        names(&mut snap, &[(4, "zsh")]);
        assert!(snap.plan_title(&config, at(3, 4)).is_empty());
        snap.tabs.push(tab_named(3, 2, "c", true));
        assert_eq!(
            snap.plan_title(&config, at(3, 4)),
            vec![TitleOp::Rename(4, "\u{200b}P · a · b · [c]".into())]
        );
    }

    #[test]
    fn an_unknown_focus_keeps_the_title() {
        let mut snap = title_snapshot();
        let config = titled_config();
        names(&mut snap, &[(4, A1)]);
        snap.plan_title(&config, at(1, 4));
        assert!(snap.plan_title(&config, Focused::Unknown).is_empty());
        assert!(snap.titled.contains_key(&4));
        // and it didn't count as leaving the pane
        assert!(snap.plan_title(&config, at(1, 4)).is_empty());
    }

    #[test]
    fn title_is_off_or_silent_when_frozen() {
        let mut snap = title_snapshot();
        names(&mut snap, &[(4, "zsh"), (5, "zsh")]);
        assert!(snap.plan_title(&Config::default(), at(1, 4)).is_empty());
        let config = titled_config();
        snap.plan_title(&config, at(1, 4));
        snap.freeze();
        assert!(snap.plan_title(&config, at(1, 5)).is_empty());
        assert!(snap.plan_tab_names(&config).is_empty());
        // the hint survives, so the next instance of this client looks at the pane
        assert!(snap.titled.contains_key(&4));
    }

    #[test]
    fn state_and_name_matching_never_see_a_marked_name() {
        let mut snap = title_snapshot();
        snap.tabs[0].name = A2.into();
        snap.tabs[1].name = "Tab #2\u{200b}".into();
        let state = snap.state();
        assert_eq!(state.tabs[0].name, "a");
        assert_eq!(state.tabs[1].name, "Tab #2");
        assert_eq!(
            plan_bind(&Target::Name("a".into()), &snap.tabs),
            BindStep::GoTo(1)
        );
        assert_eq!(
            plan_bind(&Target::Name("Tab #2".into()), &snap.tabs),
            BindStep::GoTo(2)
        );
        assert_eq!(
            plan_bind(&Target::Name(A1.into()), &snap.tabs),
            BindStep::GoTo(1)
        );
    }

    #[test]
    fn the_leader_gives_a_tab_that_tracks_our_title_its_name_back() {
        let config = titled_config();
        let mut snap = title_snapshot();
        assert!(snap.plan_tab_names(&config).is_empty());
        snap.tabs[0].name = A2.into();
        snap.tabs[1].name = "mine".into();
        assert_eq!(snap.plan_tab_names(&config), vec![(1, "a".to_owned())]);
        // the name it would have by default needs a mark behind it to stop the tracking
        snap.tabs[0].name = "\u{200b}P · [Tab #2] · b".into();
        assert_eq!(
            snap.plan_tab_names(&config),
            vec![(1, "Tab #2\u{200b}".to_owned())]
        );
        // a cut title: the default name
        snap.tabs[0].name = "\u{200b}P · [a…".into();
        assert_eq!(
            snap.plan_tab_names(&config),
            vec![(1, "Tab #2\u{200b}".to_owned())]
        );
        // only the leader renames, and only with the option on
        snap.tabs[1].other_clients = Vec::new();
        snap.own_client = 3;
        snap.tabs[0].other_clients = vec![2];
        assert!(snap.plan_tab_names(&config).is_empty());
        snap.own_client = 1;
        assert!(snap.plan_tab_names(&Config::default()).is_empty());
    }
}

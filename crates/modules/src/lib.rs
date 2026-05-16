//! Januas ADE module registry + instance set (S5.5e).
//!
//! Purpose: a module is a code unit that gives the user a way to work
//! (Parallex = project + grid + AI providers; future ones might be an
//! agentic team-of-agents pane, a reading queue, etc.). The subway tabs
//! are running instances of modules, dynamically spawned by the user.
//! Home is NOT a module — it's a distinct button to the left of the tab
//! divider that returns to the launcher view. This crate models the
//! module-kind enum (the registry), the per-instance state, the ordered
//! instance list, and which thing is currently selected (Home or one of
//! the instance tabs).
//! Public surface: [`ModuleKind`], [`InstanceId`], [`ModuleInstance`],
//! [`Selection`], [`InstanceSet`], [`InstanceSetError`].
//! Why this crate (vs a `modules.rs` module under `crates/ui/`): the
//! module registry is domain data and will be consumed by the binary's
//! main loop, the future Parallex/Agentic crates, the eventual CLI, and
//! the persistence layer. Keeping it UI-free preserves the one-way
//! `shared → features → pages` import direction.
//! Not responsibilities: rendering (`januas-ui`), pointer state
//! (`januas-ui::pointer`), per-module business logic (each module's own
//! crate, landing S6+ for Parallex), persistence (deferred).
//! Test strategy: `tests/instance_set.rs` covers spawn → tab appears,
//! close → tab gone + selection snaps to Home, select(Tab(id)), and the
//! Home-isn't-a-tab invariant.

use std::time::SystemTime;

/// Static module-kind enum — the registry.
///
/// Adding a new module = adding a variant + extending `ALL` + filling in the
/// trio of metadata methods. No other code in this crate touches kind-
/// specific state at v0.1; that lives in each module's own crate (Parallex
/// arrives at S6).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ModuleKind {
    /// The project-picker + grid-composer + provider-auto-launcher
    /// workspace mode. Locked as the v0.1 module per
    /// `~/Desktop/Januas/docs/brand.md`.
    Parallex,
}

impl ModuleKind {
    /// Every registered module, in launcher / dropdown display order.
    pub const ALL: &'static [Self] = &[Self::Parallex];

    /// Display name shown on the launcher button and in the tab label
    /// when the instance hasn't been renamed.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Parallex => "Parallex",
        }
    }

    /// One-line description shown beneath the launcher button on Home.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Parallex => "project · grid · providers",
        }
    }

    /// Mono glyph rendered as the module's launcher icon. Chosen to read
    /// as a typographic mark rather than an emoji (No-slop rule).
    #[must_use]
    pub const fn icon(self) -> &'static str {
        match self {
            Self::Parallex => "▦",
        }
    }

    /// Default brand-color sRGB byte triple. New instances pick this up
    /// unless the spawn caller overrides it. Locked to the Januas
    /// workspace-dot palette from `~/Desktop/Januas/docs/design-tokens.md`.
    #[must_use]
    pub const fn default_color(self) -> [u8; 3] {
        match self {
            // Accent (Janus teal) — the locked primary highlight.
            Self::Parallex => [0x6b, 0xb8, 0xa8],
        }
    }
}

/// Stable instance identifier — monotonic per [`InstanceSet`].
///
/// Newtyped so callers don't accidentally pass a node id (`u32`) or
/// a workspace-color byte (also `u32`-shaped after upcasting).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId(pub u32);

impl InstanceId {
    /// Convert to a `u32` for callers that need the raw number — e.g.
    /// the `januas-ui` crate's `NodeId` alias.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

/// One running instance of a registered module — what a subway tab
/// actually represents.
///
/// `name` defaults to the kind's `name()` and can be renamed by the
/// caller. `color` defaults to the kind's `default_color()` and can be
/// overridden too. Kind-specific runtime state (cwd, grid layout, etc.
/// for Parallex) lives in the module's own crate, not here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleInstance {
    /// Stable id, unique within the owning [`InstanceSet`].
    pub id: InstanceId,
    /// Which module this is an instance of.
    pub kind: ModuleKind,
    /// Tab label.
    pub name: String,
    /// sRGB byte triple — the workspace dot in the subway, the leading
    /// accent on the launcher row, etc.
    pub color: [u8; 3],
    /// Creation timestamp.
    pub created_at: SystemTime,
}

/// What's currently active.
///
/// `Home` shows the launcher view (logo + wordmark + module-launcher
/// buttons). `Tab(InstanceId)` shows the module's own scene for that
/// instance. Modeled as an enum (vs `Option<InstanceId>` + a bool)
/// because the two states are exclusive and the enum is exhaustive
/// over the actual product behavior.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    /// Home view — the launcher scene, no specific module instance.
    Home,
    /// A specific module-instance tab is active.
    Tab(InstanceId),
}

/// Reasons [`InstanceSet`] mutators can refuse a call.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InstanceSetError {
    /// The instance id doesn't exist in this set.
    NotFound,
    /// `move_to` received an out-of-range target index.
    InvalidPosition,
}

impl core::fmt::Display for InstanceSetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotFound => f.write_str("instance id not found"),
            Self::InvalidPosition => f.write_str("move_to target index out of range"),
        }
    }
}

impl std::error::Error for InstanceSetError {}

/// The ordered list of running module instances plus the current
/// [`Selection`]. Home is intentionally external — it's a fixed UI
/// element that lives outside the tab strip in the subway.
///
/// Invariants enforced by the public methods:
/// 1. Every `InstanceId` is unique within the set.
/// 2. `instances` order is the display order (leftmost = first).
/// 3. When `selected == Selection::Tab(id)`, `id` is always in this set.
#[derive(Clone, Debug)]
pub struct InstanceSet {
    instances: Vec<ModuleInstance>,
    selected: Selection,
    next_id: u32,
}

impl Default for InstanceSet {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceSet {
    /// Boot state: no tabs, Home selected.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            instances: Vec::new(),
            selected: Selection::Home,
            next_id: 1,
        }
    }

    /// Spawn a new instance of the given module kind. Returns its
    /// freshly-issued id; appends to the end of the tab order.
    ///
    /// The new instance is **not** auto-selected — caller decides via
    /// [`Self::select`] whether the spawn should swap the view. Typical
    /// pattern is `let id = set.spawn(kind); set.select(Selection::Tab(id))?;`.
    pub fn spawn(&mut self, kind: ModuleKind) -> InstanceId {
        let id = InstanceId(self.next_id);
        self.next_id += 1;
        self.instances.push(ModuleInstance {
            id,
            kind,
            name: kind.name().to_string(),
            color: kind.default_color(),
            created_at: SystemTime::now(),
        });
        id
    }

    /// Close (remove) an instance. If the closed instance was selected,
    /// selection snaps back to Home.
    ///
    /// # Errors
    ///
    /// Returns [`InstanceSetError::NotFound`] if `id` isn't in this set.
    pub fn close(&mut self, id: InstanceId) -> Result<ModuleInstance, InstanceSetError> {
        let pos = self
            .instances
            .iter()
            .position(|inst| inst.id == id)
            .ok_or(InstanceSetError::NotFound)?;
        let removed = self.instances.remove(pos);
        if self.selected == Selection::Tab(id) {
            self.selected = Selection::Home;
        }
        Ok(removed)
    }

    /// Iterate instances in display order. Leftmost subway tab is first.
    pub fn ordered(&self) -> impl Iterator<Item = &ModuleInstance> {
        self.instances.iter()
    }

    /// Look up an instance by id.
    #[must_use]
    pub fn get(&self, id: InstanceId) -> Option<&ModuleInstance> {
        self.instances.iter().find(|inst| inst.id == id)
    }

    /// Mutable variant of [`Self::get`]. Use for rename / color edits.
    pub fn get_mut(&mut self, id: InstanceId) -> Option<&mut ModuleInstance> {
        self.instances.iter_mut().find(|inst| inst.id == id)
    }

    /// Current selection.
    #[must_use]
    pub const fn selected(&self) -> Selection {
        self.selected
    }

    /// Resolve [`Self::selected`] to its `&ModuleInstance`, or `None`
    /// when Home is selected.
    #[must_use]
    pub fn selected_instance(&self) -> Option<&ModuleInstance> {
        match self.selected {
            Selection::Home => None,
            Selection::Tab(id) => self.get(id),
        }
    }

    /// Change the current selection.
    ///
    /// # Errors
    ///
    /// Returns [`InstanceSetError::NotFound`] when called with
    /// [`Selection::Tab`] for an id that isn't in this set.
    pub fn select(&mut self, selection: Selection) -> Result<Selection, InstanceSetError> {
        if let Selection::Tab(id) = selection
            && !self.instances.iter().any(|inst| inst.id == id)
        {
            return Err(InstanceSetError::NotFound);
        }
        self.selected = selection;
        Ok(selection)
    }

    /// Move an instance to position `target` in the tab order.
    ///
    /// # Errors
    ///
    /// [`InstanceSetError::NotFound`] when `id` isn't present;
    /// [`InstanceSetError::InvalidPosition`] when `target >= len()`.
    pub fn move_to(&mut self, id: InstanceId, target: usize) -> Result<(), InstanceSetError> {
        let current = self
            .instances
            .iter()
            .position(|inst| inst.id == id)
            .ok_or(InstanceSetError::NotFound)?;
        if target >= self.instances.len() {
            return Err(InstanceSetError::InvalidPosition);
        }
        let inst = self.instances.remove(current);
        self.instances.insert(target, inst);
        Ok(())
    }

    /// Number of running instances (excludes Home — Home isn't a tab).
    #[must_use]
    pub const fn len(&self) -> usize {
        self.instances.len()
    }

    /// `true` when no instances exist and the subway shows only the Home
    /// button + divider + the `+` slot.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }
}

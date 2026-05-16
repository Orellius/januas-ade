//! Integration tests for the module-instance set (S5.5e).
//!
//! Boot state, spawn, select, close (incl. selection-snap-to-Home), and
//! the `move_to` ordering rule. Home is intentionally NOT modeled in the
//! set — selecting `Home` is the absence of a tab selection.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap/expect to keep assertions terse — a failed unwrap is a test failure"
)]

use januas_modules::{InstanceSet, InstanceSetError, ModuleKind, Selection};

#[test]
fn boot_state_is_home_with_no_tabs() {
    let set = InstanceSet::new();
    assert!(set.is_empty());
    assert_eq!(set.len(), 0);
    assert_eq!(set.selected(), Selection::Home);
    assert!(set.selected_instance().is_none());
}

#[test]
fn module_kind_registry_has_exactly_one_v0_1_module() {
    // Guard against accidentally adding a variant to `ModuleKind` without
    // updating every downstream consumer (subway dropdown, scene swap,
    // hand-off table). When v0.2 lands a second module this assertion
    // bumps to 2 and is the trigger to walk the call sites.
    assert_eq!(ModuleKind::ALL.len(), 1);
    assert_eq!(ModuleKind::ALL[0], ModuleKind::Parallex);
}

#[test]
fn spawn_appends_an_instance() {
    let mut set = InstanceSet::new();
    let id = set.spawn(ModuleKind::Parallex);
    assert_eq!(set.len(), 1);
    let inst = set.get(id).expect("just spawned");
    assert_eq!(inst.kind, ModuleKind::Parallex);
    assert_eq!(inst.name, "Parallex");
    assert_eq!(inst.color, ModuleKind::Parallex.default_color());
}

#[test]
fn spawn_does_not_auto_select() {
    let mut set = InstanceSet::new();
    let _id = set.spawn(ModuleKind::Parallex);
    assert_eq!(set.selected(), Selection::Home);
}

#[test]
fn select_tab_changes_selection() {
    let mut set = InstanceSet::new();
    let id = set.spawn(ModuleKind::Parallex);
    set.select(Selection::Tab(id)).unwrap();
    assert_eq!(set.selected(), Selection::Tab(id));
    assert_eq!(set.selected_instance().map(|i| i.id), Some(id));
}

#[test]
fn select_unknown_tab_returns_not_found() {
    let mut set = InstanceSet::new();
    let err = set
        .select(Selection::Tab(januas_modules::InstanceId(99)))
        .unwrap_err();
    assert_eq!(err, InstanceSetError::NotFound);
}

#[test]
fn close_removes_instance_and_snaps_to_home_when_selected() {
    let mut set = InstanceSet::new();
    let id_a = set.spawn(ModuleKind::Parallex);
    let id_b = set.spawn(ModuleKind::Parallex);
    set.select(Selection::Tab(id_b)).unwrap();
    set.close(id_b).unwrap();
    assert_eq!(set.len(), 1);
    assert!(set.get(id_b).is_none());
    assert!(set.get(id_a).is_some());
    assert_eq!(set.selected(), Selection::Home);
}

#[test]
fn close_unrelated_tab_keeps_current_selection() {
    let mut set = InstanceSet::new();
    let id_a = set.spawn(ModuleKind::Parallex);
    let id_b = set.spawn(ModuleKind::Parallex);
    set.select(Selection::Tab(id_a)).unwrap();
    set.close(id_b).unwrap();
    assert_eq!(set.selected(), Selection::Tab(id_a));
}

#[test]
fn close_unknown_id_errors() {
    let mut set = InstanceSet::new();
    let err = set.close(januas_modules::InstanceId(99)).unwrap_err();
    assert_eq!(err, InstanceSetError::NotFound);
}

#[test]
fn move_to_reorders_instances() {
    let mut set = InstanceSet::new();
    let id_a = set.spawn(ModuleKind::Parallex);
    let id_b = set.spawn(ModuleKind::Parallex);
    let id_c = set.spawn(ModuleKind::Parallex);

    // Move the first tab to last position. The full new ordering should be
    // [B, C, A] — anything else means move_to scrambled tabs that weren't
    // supposed to shift.
    set.move_to(id_a, 2).unwrap();
    let order: Vec<_> = set.ordered().map(|i| i.id).collect();
    assert_eq!(order, vec![id_b, id_c, id_a]);
}

#[test]
fn move_to_survives_close_of_unrelated_tab() {
    let mut set = InstanceSet::new();
    let id_a = set.spawn(ModuleKind::Parallex);
    let id_b = set.spawn(ModuleKind::Parallex);
    let id_c = set.spawn(ModuleKind::Parallex);

    // Close the middle tab, then reorder the survivors.
    set.close(id_b).unwrap();
    set.move_to(id_a, 1).unwrap();
    let order: Vec<_> = set.ordered().map(|i| i.id).collect();
    assert_eq!(order, vec![id_c, id_a]);
}

#[test]
fn close_active_then_select_another_tab() {
    let mut set = InstanceSet::new();
    let id_a = set.spawn(ModuleKind::Parallex);
    let id_b = set.spawn(ModuleKind::Parallex);

    // Common "close active tab, then click another" flow.
    set.select(Selection::Tab(id_a)).unwrap();
    set.close(id_a).unwrap();
    assert_eq!(set.selected(), Selection::Home);
    set.select(Selection::Tab(id_b)).unwrap();
    assert_eq!(set.selected(), Selection::Tab(id_b));
}

#[test]
fn move_to_invalid_position_errors() {
    let mut set = InstanceSet::new();
    let id = set.spawn(ModuleKind::Parallex);
    let err = set.move_to(id, 99).unwrap_err();
    assert_eq!(err, InstanceSetError::InvalidPosition);
}

#[test]
fn rename_via_get_mut() {
    let mut set = InstanceSet::new();
    let id = set.spawn(ModuleKind::Parallex);
    set.get_mut(id).unwrap().name = "Personal".to_string();
    assert_eq!(set.get(id).unwrap().name, "Personal");
}

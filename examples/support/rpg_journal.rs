//! Transient quest journal. Gameplay and portable snapshots do not depend on it.
use super::QuestState;
use titan::render::{Color, Image, ImageAssets, ImageId, RenderFrame, SpriteDraw};
use titan::ui::{UiButton, UiColumn, UiFocus, UiNode, UiPointer, UiText};
use titan::{Component, Entity, Name, Query, Res, World};

#[derive(Component)]
pub struct JournalNode;

pub(super) struct Journal {
    open: bool,
    selected: usize,
    focus: UiFocus,
    physical: UiPointer,
    controlled: UiPointer,
    opener: Entity,
    buttons: [Entity; 3],
    detail: Entity,
    panel: ImageId,
    highlight: ImageId,
}

pub fn setup(world: &mut World) {
    let opener = world.iter::<super::QuestHud>().next().unwrap().0;
    world.insert(opener, UiButton::default()).unwrap();
    let assets = world.resource_mut::<ImageAssets>().unwrap();
    let panel = assets.insert(
        Image::from_fn(144, 90, |x, y| {
            if x == 0 || y == 0 || x == 143 || y == 89 {
                Color::rgb(209, 187, 117)
            } else if x == 1 || y == 1 || x == 142 || y == 88 {
                Color::rgb(75, 91, 66)
            } else {
                Color::rgb(27, 43, 36)
            }
        })
        .unwrap(),
    );
    let highlight = assets.insert(
        Image::from_fn(130, 10, |x, _| {
            if x < 2 {
                Color::rgb(245, 214, 131)
            } else {
                Color::rgb(65, 85, 57)
            }
        })
        .unwrap(),
    );
    let mut title = UiNode::new(16, 21, 128, 7);
    title.layer = 201;
    world.spawn_with((
        Name::new("ui/journal/title"),
        JournalNode,
        title,
        UiText::new("QUEST JOURNAL").with_color(Color::rgb(245, 214, 131)),
    ));
    let mut column = UiColumn::new(16, 33, 128, 3);
    let spawn_button = |world: &mut World, name: &str, text: &str, mut node: UiNode| {
        node.layer = 201;
        world.spawn_with((
            Name::new(name),
            JournalNode,
            node,
            UiText::new(text),
            UiButton::default(),
        ))
    };
    let shards = spawn_button(world, "ui/journal/shards", "SHARDS", column.next_node(10));
    let shrine = spawn_button(world, "ui/journal/shrine", "SHRINE", column.next_node(10));
    let mut detail_node = column.next_node(24);
    detail_node.layer = 201;
    let detail = world.spawn_with((
        Name::new("ui/journal/detail"),
        JournalNode,
        detail_node,
        UiText::new("")
            .with_wrap()
            .with_color(Color::rgb(196, 209, 177)),
    ));
    let close = spawn_button(world, "ui/journal/close", "CLOSE", column.next_node(10));
    world.insert_resource(Journal {
        open: false,
        selected: 0,
        focus: UiFocus::default(),
        physical: UiPointer::default(),
        controlled: UiPointer::default(),
        opener,
        buttons: [shards, shrine, close],
        detail,
        panel,
        highlight,
    });
    set_open(world, false);
}

pub fn is_open(world: &World) -> bool {
    world.resource::<Journal>().is_some_and(|j| j.open)
}

pub fn sync(world: &mut World) {
    let Some(j) = world.resource::<Journal>() else {
        return;
    };
    let (selected, buttons, detail) = (j.selected, j.buttons, j.detail);
    let quest = world.resource::<QuestState>().unwrap();
    let shards = quest.collected_shards;
    let active = quest.shrine_active;
    for (entity, text) in label_text(selected, buttons, detail, shards, active) {
        if let Some(label) = world.get_mut::<UiText>(entity) {
            label.text = text;
        }
    }
}

fn label_text(
    selected: usize,
    buttons: [Entity; 3],
    detail: Entity,
    shards: usize,
    active: bool,
) -> [(Entity, String); 3] {
    [
        (buttons[0], format!("SHARDS {shards}/3")),
        (
            buttons[1],
            format!("SHRINE {}", if active { "ACTIVE" } else { "WAITING" }),
        ),
        (
            detail,
            if selected == 0 {
                if shards >= 3 {
                    "ALL THREE SHARDS FOUND. VISIT THE SHRINE."
                } else {
                    "FIND THREE GOLDEN SHARDS IN THE MEADOW."
                }
            } else if active {
                "THE SHRINE IS AWAKE. YOUR QUEST IS COMPLETE."
            } else {
                "COLLECT THREE SHARDS TO AWAKEN THE SHRINE."
            }
            .to_owned(),
        ),
    ]
}

pub(super) fn sync_labels(
    quest: Res<QuestState>,
    journal: Res<Journal>,
    mut labels: Query<(&mut UiText, &JournalNode)>,
) {
    let text = label_text(
        journal.selected,
        journal.buttons,
        journal.detail,
        quest.collected_shards,
        quest.shrine_active,
    );
    labels.for_each(|entity, (label, _)| {
        if let Some((_, value)) = text.iter().find(|(target, _)| *target == entity) {
            label.text.clone_from(value);
        }
    });
}

pub fn cancel(world: &mut World) {
    if let Some(j) = world.resource_mut::<Journal>() {
        j.physical.cancel();
        j.controlled.cancel();
        j.focus.clear();
    }
}

pub fn set_open(world: &mut World, open: bool) {
    let Some(j) = world.resource_mut::<Journal>() else {
        return;
    };
    j.open = open;
    j.physical.cancel();
    j.controlled.cancel();
    j.focus.clear();
    let nodes: Vec<_> = world
        .iter::<JournalNode>()
        .map(|(entity, _)| entity)
        .collect();
    for entity in nodes {
        if let Some(node) = world.get_mut::<UiNode>(entity) {
            node.visible = open;
        }
    }
    sync(world);
    if open {
        let mut j = world.remove_resource::<Journal>().unwrap();
        j.focus.set(world, &j.buttons, j.buttons[j.selected]);
        world.insert_resource(j);
    }
}

pub fn reset(world: &mut World) {
    if let Some(j) = world.resource_mut::<Journal>() {
        j.selected = 0;
    }
    set_open(world, false);
}

fn activate(world: &mut World, entity: Entity) {
    let j = world.resource_mut::<Journal>().unwrap();
    if entity == j.opener {
        set_open(world, true);
    } else if entity == j.buttons[2] {
        set_open(world, false);
    } else if let Some(index) = j.buttons[..2].iter().position(|e| *e == entity) {
        j.selected = index;
        sync(world);
    }
}

pub fn key(world: &mut World, name: &str) -> bool {
    if name == "toggle" {
        set_open(world, !is_open(world));
        return true;
    }
    if !is_open(world) {
        return false;
    }
    if name == "close" {
        set_open(world, false);
        return true;
    }
    let mut j = world.remove_resource::<Journal>().unwrap();
    let target = match name {
        "next" | "previous" => {
            let target = j.focus.navigate(world, &j.buttons, name == "previous");
            if let Some(index) = target.and_then(|e| j.buttons[..2].iter().position(|b| *b == e)) {
                j.selected = index;
            }
            None
        }
        "activate" => j.focus.activate(world, &j.buttons),
        _ => None,
    };
    world.insert_resource(j);
    if let Some(target) = target {
        activate(world, target);
    }
    sync(world);
    true
}

pub fn pointer(
    world: &mut World,
    point: Option<(i32, i32)>,
    pressed: bool,
    physical: bool,
) -> bool {
    let mut j = world.remove_resource::<Journal>().unwrap();
    let was_open = j.open;
    let result = if physical {
        j.physical.update(world, point, pressed)
    } else {
        j.controlled.update(world, point, pressed)
    };
    if let Some(target) = result.activated {
        j.focus.set(world, &j.buttons, target);
    }
    world.insert_resource(j);
    if let Some(target) = result.activated {
        activate(world, target);
    }
    was_open || result.consumed
}

pub fn state(world: &World) -> serde_json::Value {
    let j = world.resource::<Journal>().unwrap();
    let focused = j
        .focus
        .focused()
        .and_then(|e| world.get::<Name>(e))
        .map(|name| name.as_str());
    serde_json::json!({"open": j.open, "selected": if j.selected == 0 {"shards"} else {"shrine"}, "focused": focused})
}

pub fn append_background(world: &World, frame: &mut RenderFrame) {
    let j = world.resource::<Journal>().unwrap();
    if !j.open {
        return;
    }
    frame.push(SpriteDraw::new(j.panel, 8, 14).with_layer(200));
    if let Some(node) = j
        .focus
        .focused()
        .and_then(|e| world.get::<UiNode>(e))
        .filter(|node| node.visible)
    {
        frame.push(
            SpriteDraw::new(j.highlight, node.x - 3, node.y - 2)
                .with_layer(200)
                .with_order(1),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn game() -> titan::App {
        let mut app = super::super::build_game();
        app.advance_fixed(0);
        app
    }

    #[test]
    fn closed_journal_preserves_reference_and_open_view_is_canonicalized() {
        let mut app = game();
        super::super::replay(&mut app, &super::super::recorded_walk());
        let expected = super::super::render_image(app.world()).unwrap();
        assert_eq!(super::super::image_checksum(&expected), 0xf7a298f62ad75c1c);
        set_open(app.world_mut(), true);
        assert_ne!(
            super::super::render_image(app.world()).unwrap().pixels(),
            expected.pixels()
        );
        assert_eq!(
            super::super::render_replay_image(app.world())
                .unwrap()
                .pixels(),
            expected.pixels()
        );
        reset(app.world_mut());
        assert_eq!(
            super::super::render_image(app.world()).unwrap().pixels(),
            expected.pixels()
        );
    }

    #[test]
    fn keyboard_and_pointer_select_same_objective_and_close() {
        let mut app = game();
        assert!(!key(app.world_mut(), "next"));
        key(app.world_mut(), "toggle");
        key(app.world_mut(), "next");
        assert_eq!(state(app.world())["selected"], "shrine");
        key(app.world_mut(), "previous");
        assert_eq!(state(app.world())["selected"], "shards");
        assert!(pointer(app.world_mut(), Some((20, 48)), true, true));
        assert!(pointer(app.world_mut(), Some((20, 48)), false, true));
        assert_eq!(state(app.world())["selected"], "shrine");
        key(app.world_mut(), "next");
        key(app.world_mut(), "activate");
        assert!(!is_open(app.world()));
    }

    #[test]
    fn separate_pointer_sources_and_cancel_do_not_combine_gestures() {
        let mut app = game();
        assert!(pointer(app.world_mut(), Some((8, 6)), true, true));
        pointer(app.world_mut(), Some((8, 6)), false, false);
        assert!(!is_open(app.world()));
        cancel(app.world_mut());
        pointer(app.world_mut(), Some((8, 6)), false, true);
        assert!(!is_open(app.world()));
        pointer(app.world_mut(), Some((8, 6)), true, false);
        pointer(app.world_mut(), Some((8, 6)), false, false);
        assert!(is_open(app.world()));
    }
    #[test]
    fn canceled_focus_recovers_and_disabled_or_hidden_close_cannot_activate() {
        let mut app = game();
        set_open(app.world_mut(), true);
        cancel(app.world_mut());
        assert!(state(app.world())["focused"].is_null());
        key(app.world_mut(), "next");
        assert_eq!(state(app.world())["focused"], "ui/journal/shards");
        key(app.world_mut(), "previous");
        let close = app.world().resource::<Journal>().unwrap().buttons[2];
        app.world_mut().get_mut::<UiButton>(close).unwrap().enabled = false;
        key(app.world_mut(), "activate");
        assert!(is_open(app.world()));
        app.world_mut().get_mut::<UiButton>(close).unwrap().enabled = true;
        key(app.world_mut(), "previous");
        app.world_mut().get_mut::<UiNode>(close).unwrap().visible = false;
        key(app.world_mut(), "activate");
        assert!(is_open(app.world()));
        assert!(!app.world().get::<UiNode>(close).unwrap().visible);
    }
}

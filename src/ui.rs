//! Entity-based, fixed-pixel UI shared by interactive and headless games.
//!
//! A game spawns [`UiNode`] and [`UiText`] components, updates their ordinary ECS
//! data, and calls [`append_ui`] from its frame extractor. Add [`UiButton`] to
//! make a node consume primary-pointer gestures. Games own button actions and
//! update schedules; hosts own pointer IDs and surface-coordinate mapping.
//! There is no implicit layout, wrapping, parenting, focus traversal or clipping.

use std::collections::BTreeMap;

use crate::inspection::Inspector;
use crate::render::{Color, Image, ImageAssets, ImageId, RenderFrame, SpriteDraw};
use crate::{Component, Entity, World};
use titan_protocol::{FieldMetadata, ProtocolError};

/// Framebuffer-pixel origin and hit rectangle. Text is not clipped to the bounds.
/// Higher `(layer, order, entity)` sorts later in drawing and wins button hits.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiNode {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub layer: i32,
    pub order: i32,
    pub visible: bool,
}

impl UiNode {
    /// Creates a visible node on layer 100, above default-layer world sprites.
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            layer: 100,
            order: 0,
            visible: true,
        }
    }

    /// Half-open bounds: the right and bottom edges are outside the node.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        let dx = i64::from(x) - i64::from(self.x);
        let dy = i64::from(y) - i64::from(self.y);
        self.visible
            && dx >= 0
            && dy >= 0
            && dx < i64::from(self.width)
            && dy < i64::from(self.height)
    }
}

/// Text using the world's [`BitmapFont`]. Unknown glyphs advance without drawing.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct UiText {
    pub text: String,
    pub color: Color,
}

impl UiText {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: Color::WHITE,
        }
    }

    pub const fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
}

/// A pointer target. Disabled visible buttons consume input but never activate.
/// The game decides how to render enabled and disabled button states.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiButton {
    pub enabled: bool,
}

impl Default for UiButton {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// A small image-backed font resource. Images live in the world's `ImageAssets`.
/// One font is used per extraction; games can construct their own glyph map.
pub struct BitmapFont {
    glyphs: BTreeMap<char, ImageId>,
    advance: u32,
    line_height: u32,
}

impl BitmapFont {
    /// Glyphs are tinted by `UiText::color`. Advance and line height must be positive.
    pub fn new(glyphs: BTreeMap<char, ImageId>, advance: u32, line_height: u32) -> Self {
        assert!(
            advance > 0 && line_height > 0,
            "font spacing must be positive"
        );
        Self {
            glyphs,
            advance,
            line_height,
        }
    }

    /// Generates reusable 3×5 white glyph masks with four-pixel character advance.
    /// Existing arena glyph shapes are preserved; text color is supplied per entity.
    pub fn tiny(assets: &mut ImageAssets) -> Self {
        let mut glyphs = BTreeMap::new();
        for (character, rows) in [
            ('0', [7, 5, 5, 5, 7]),
            ('1', [2, 6, 2, 2, 7]),
            ('2', [7, 1, 7, 4, 7]),
            ('3', [7, 1, 7, 1, 7]),
            ('4', [5, 5, 7, 1, 1]),
            ('5', [7, 4, 7, 1, 7]),
            ('6', [7, 4, 7, 5, 7]),
            ('7', [7, 1, 1, 1, 1]),
            ('8', [7, 5, 7, 5, 7]),
            ('9', [7, 5, 7, 1, 7]),
            ('A', [2, 5, 7, 5, 5]),
            ('B', [6, 5, 6, 5, 6]),
            ('C', [3, 4, 4, 4, 3]),
            ('D', [6, 5, 5, 5, 6]),
            ('E', [7, 4, 6, 4, 7]),
            ('F', [7, 4, 6, 4, 4]),
            ('G', [3, 4, 5, 5, 3]),
            ('H', [5, 5, 7, 5, 5]),
            ('I', [7, 2, 2, 2, 7]),
            ('J', [1, 1, 1, 5, 2]),
            ('K', [5, 5, 6, 5, 5]),
            ('L', [4, 4, 4, 4, 7]),
            ('M', [5, 7, 7, 5, 5]),
            ('N', [5, 7, 7, 7, 5]),
            ('O', [7, 5, 5, 5, 7]),
            ('P', [6, 5, 6, 4, 4]),
            ('Q', [7, 5, 5, 7, 1]),
            ('R', [6, 5, 6, 5, 5]),
            ('S', [7, 4, 7, 1, 7]),
            ('T', [7, 2, 2, 2, 2]),
            ('U', [5, 5, 5, 5, 7]),
            ('V', [5, 5, 5, 5, 2]),
            ('W', [5, 5, 7, 7, 5]),
            ('X', [5, 5, 2, 5, 5]),
            ('Y', [5, 5, 2, 2, 2]),
            ('Z', [7, 1, 2, 4, 7]),
            ('.', [0, 0, 0, 0, 2]),
            ('/', [1, 1, 2, 4, 4]),
        ] {
            let image = Image::from_fn(3, 5, |x, y| {
                if rows[y as usize] & (1 << (2 - x)) != 0 {
                    Color::WHITE
                } else {
                    Color::TRANSPARENT
                }
            })
            .expect("fixed-size glyph image");
            glyphs.insert(character, assets.insert(image));
        }
        Self::new(glyphs, 4, 7)
    }

    pub fn glyph(&self, character: char) -> Option<ImageId> {
        self.glyphs.get(&character).copied()
    }
}

/// Appends renderer-neutral glyph sprites without creating or mutating entities.
/// Missing fonts yield no text. Missing image assets remain renderer errors.
/// Newlines advance one font line; there is no wrapping or bounds clipping.
pub fn append_ui(world: &World, frame: &mut RenderFrame) {
    let Some(font) = world.resource::<BitmapFont>() else {
        return;
    };
    let mut nodes: Vec<_> = world
        .iter2::<UiNode, UiText>()
        .filter(|(_, node, _)| node.visible)
        .collect();
    nodes.sort_by_key(|(entity, node, _)| (node.layer, node.order, *entity));
    for (_, node, text) in nodes {
        let mut x = i64::from(node.x);
        let mut y = i64::from(node.y);
        for character in text.text.chars() {
            if character == '\n' {
                x = i64::from(node.x);
                y = y.saturating_add(i64::from(font.line_height));
                continue;
            }
            if let (Some(image), Ok(x), Ok(y)) =
                (font.glyph(character), i32::try_from(x), i32::try_from(y))
            {
                frame.push(
                    SpriteDraw::new(image, x, y)
                        .with_layer(node.layer)
                        .with_order(node.order)
                        .with_tint(text.color),
                );
            }
            x = x.saturating_add(i64::from(font.advance));
        }
    }
}

/// The topmost visible button, including disabled buttons that block input.
pub fn hit_test(world: &World, x: i32, y: i32) -> Option<Entity> {
    world
        .iter2::<UiNode, UiButton>()
        .filter(|(_, node, _)| node.contains(x, y))
        .max_by_key(|(entity, node, _)| (node.layer, node.order, *entity))
        .map(|(entity, _, _)| entity)
}

/// Result of a primary-pointer update. Activation occurs once, on release.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UiPointerResult {
    pub consumed: bool,
    pub activated: Option<Entity>,
}

/// Single-primary-pointer gesture state, independent of any window API.
/// A press inside UI remains consumed until release even if the target disappears.
/// An update observing a hidden, disabled or removed target cancels activation.
/// Dragging out and releasing cancels activation; a press outside never activates.
#[derive(Default)]
pub struct UiPointer {
    pressed: bool,
    captured: Option<Entity>,
    consumed_press: bool,
}

impl UiPointer {
    pub fn update(
        &mut self,
        world: &World,
        position: Option<(i32, i32)>,
        pressed: bool,
    ) -> UiPointerResult {
        // Once an update observes a canceled target, re-enabling or replacing it
        // cannot revive the gesture. Keep consuming its release, however.
        self.captured = self.captured.filter(|entity| {
            world
                .get::<UiNode>(*entity)
                .is_some_and(|node| node.visible)
                && world
                    .get::<UiButton>(*entity)
                    .is_some_and(|button| button.enabled)
        });
        let hit = position.and_then(|(x, y)| hit_test(world, x, y));
        let mut result = UiPointerResult {
            consumed: hit.is_some() || self.consumed_press,
            activated: None,
        };
        if pressed && !self.pressed {
            self.consumed_press = hit.is_some();
            self.captured = hit.filter(|entity| {
                world
                    .get::<UiButton>(*entity)
                    .is_some_and(|button| button.enabled)
            });
        } else if !pressed && self.pressed {
            result.activated = self.captured.filter(|entity| {
                Some(*entity) == hit
                    && world
                        .get::<UiButton>(*entity)
                        .is_some_and(|button| button.enabled)
            });
            self.captured = None;
            self.consumed_press = false;
        }
        self.pressed = pressed;
        result
    }

    /// Discards the active gesture and returns whether its press was consumed.
    /// Hosts must also forget the physical pointer ID and drop orphaned events
    /// from that canceled gesture. A subsequent fresh press works immediately.
    pub fn cancel(&mut self) -> bool {
        let consumed = self.consumed_press;
        *self = Self::default();
        consumed
    }
}

/// Maps a stretched surface's local coordinates to framebuffer pixels.
/// Surface dimensions and positions may be CSS pixels or physical pixels, but
/// must use the same units. Outside, nonfinite and zero-sized surfaces return None.
pub fn point_from_surface(
    x: f64,
    y: f64,
    surface_width: f64,
    surface_height: f64,
    logical_width: u32,
    logical_height: u32,
) -> Option<(i32, i32)> {
    if ![x, y, surface_width, surface_height]
        .into_iter()
        .all(f64::is_finite)
        || surface_width <= 0.0
        || surface_height <= 0.0
        || logical_width == 0
        || logical_height == 0
        || x < 0.0
        || y < 0.0
        || x >= surface_width
        || y >= surface_height
    {
        return None;
    }
    let x = ((x / surface_width * f64::from(logical_width)).floor() as u64)
        .min(u64::from(logical_width - 1));
    let y = ((y / surface_height * f64::from(logical_height)).floor() as u64)
        .min(u64::from(logical_height - 1));
    Some((i32::try_from(x).ok()?, i32::try_from(y).ok()?))
}

/// Exposes component data through the existing inspection protocol, read-only.
/// Call once per inspector; duplicate field registration returns a protocol error.
pub fn register_ui_inspection(inspector: &mut Inspector) -> Result<&mut Inspector, ProtocolError> {
    fn field(type_name: &str, description: &str) -> FieldMetadata {
        FieldMetadata {
            type_name: type_name.into(),
            description: description.into(),
            writable: false,
            minimum: None,
            maximum: None,
            unit: None,
        }
    }
    for (name, description, getter) in [
        (
            "x",
            "Framebuffer-pixel left edge",
            (|node: &UiNode| node.x) as fn(&UiNode) -> i32,
        ),
        (
            "y",
            "Framebuffer-pixel top edge",
            (|node: &UiNode| node.y) as fn(&UiNode) -> i32,
        ),
        (
            "layer",
            "Sprite and hit-test layer",
            (|node: &UiNode| node.layer) as fn(&UiNode) -> i32,
        ),
        (
            "order",
            "Order within the layer",
            (|node: &UiNode| node.order) as fn(&UiNode) -> i32,
        ),
    ] {
        inspector.register_read_only_field::<UiNode, i32>(
            name,
            field("i32", description),
            getter,
        )?;
    }
    inspector.register_read_only_field::<UiNode, u32>(
        "width",
        field("u32", "Hit rectangle width in pixels"),
        |node| node.width,
    )?;
    inspector.register_read_only_field::<UiNode, u32>(
        "height",
        field("u32", "Hit rectangle height in pixels"),
        |node| node.height,
    )?;
    inspector.register_read_only_field::<UiNode, bool>(
        "visible",
        field("bool", "Draw and receive pointer hits"),
        |node| node.visible,
    )?;
    inspector.register_read_only_field::<UiText, String>(
        "text",
        field("String", "Displayed text"),
        |text| text.text.clone(),
    )?;
    inspector.register_read_only_field::<UiText, [u8; 4]>(
        "color",
        field("[u8; 4]", "Straight-alpha RGBA text tint"),
        |text| {
            [
                text.color.red,
                text.color.green,
                text.color.blue,
                text.color.alpha,
            ]
        },
    )?;
    inspector.register_read_only_field::<UiButton, bool>(
        "enabled",
        field("bool", "Whether a completed gesture may activate"),
        |button| button.enabled,
    )?;
    Ok(inspector)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspection::InspectionConfig;
    use crate::render::SoftwareRenderer;
    use crate::{App, Name};
    use titan_protocol::{
        EntityId, ErrorCode, Request, RequestEnvelope, Response, ResponseOutcome,
    };

    fn button(world: &mut World, node: UiNode) -> Entity {
        world.spawn_with((node, UiButton::default()))
    }

    #[test]
    fn text_is_extracted_from_ecs_and_white_masks_preserve_hud_tint() {
        let mut world = World::new();
        let mut assets = ImageAssets::new();
        let font = BitmapFont::tiny(&mut assets);
        world.insert_resource(font);
        let tint = Color::rgb(225, 239, 249);
        let label = world.spawn_with((
            UiNode::new(2, 1, 1, 1),
            UiText::new("H?\nP").with_color(tint),
        ));
        // Bounds do not clip glyphs, and entities without both components do not draw.
        world.spawn_with(UiText::new("H"));
        let mut hidden = UiNode::new(0, 0, 3, 5);
        hidden.visible = false;
        world.spawn_with((hidden, UiText::new("H")));
        let mut frame = RenderFrame::new(12, 14, Color::BLACK);
        append_ui(&world, &mut frame);
        assert_eq!(frame.sprites().len(), 2);
        assert_eq!((frame.sprites()[1].x, frame.sprites()[1].y), (2, 8));
        let image = SoftwareRenderer::render(&frame, &assets).unwrap();
        assert_eq!(image.pixel(2, 1), Some(tint));
        assert_eq!(image.pixel(3, 1), Some(Color::BLACK));
        assert_eq!(image.pixel(4, 1), Some(tint));
        assert_eq!(image.pixel(3, 3), Some(tint));
        assert_eq!(image.pixel(0, 0), Some(Color::BLACK));

        world.get_mut::<UiText>(label).unwrap().text = "V".into();
        world.get_mut::<UiNode>(label).unwrap().x = 7;
        let mut changed = RenderFrame::new(12, 14, Color::BLACK);
        append_ui(&world, &mut changed);
        let image = SoftwareRenderer::render(&changed, &assets).unwrap();
        assert_eq!(image.pixel(2, 1), Some(Color::BLACK));
        assert_eq!(image.pixel(7, 1), Some(tint));
        assert_eq!(image.pixel(8, 5), Some(tint));
    }

    #[test]
    fn paint_and_hit_order_agree_after_dense_storage_changes() {
        let mut world = World::new();
        let mut assets = ImageAssets::new();
        world.insert_resource(BitmapFont::tiny(&mut assets));
        let removed = button(&mut world, UiNode::new(0, 0, 3, 5));
        let lower = world.spawn_with((
            UiNode::new(0, 0, 3, 5),
            UiText::new("H").with_color(Color::rgb(255, 0, 0)),
            UiButton::default(),
        ));
        let upper = world.spawn_with((
            UiNode::new(0, 0, 3, 5),
            UiText::new("H").with_color(Color::rgb(0, 255, 0)),
            UiButton::default(),
        ));
        world.despawn(removed);
        assert_eq!(hit_test(&world, 0, 0), Some(upper));
        let mut frame = RenderFrame::new(3, 5, Color::BLACK);
        append_ui(&world, &mut frame);
        assert_eq!(
            SoftwareRenderer::render(&frame, &assets)
                .unwrap()
                .pixel(0, 0),
            Some(Color::rgb(0, 255, 0))
        );
        world.get_mut::<UiNode>(lower).unwrap().order = 1;
        assert_eq!(hit_test(&world, 0, 0), Some(lower));
        let mut frame = RenderFrame::new(3, 5, Color::BLACK);
        append_ui(&world, &mut frame);
        assert_eq!(
            SoftwareRenderer::render(&frame, &assets)
                .unwrap()
                .pixel(0, 0),
            Some(Color::rgb(255, 0, 0))
        );
        world.get_mut::<UiNode>(upper).unwrap().layer = 101;
        assert_eq!(hit_test(&world, 0, 0), Some(upper));
        let mut frame = RenderFrame::new(3, 5, Color::BLACK);
        append_ui(&world, &mut frame);
        assert_eq!(
            SoftwareRenderer::render(&frame, &assets)
                .unwrap()
                .pixel(0, 0),
            Some(Color::rgb(0, 255, 0))
        );
    }

    #[test]
    fn pointer_activates_once_on_inside_release_and_cancels_outside() {
        let mut world = World::new();
        let target = button(&mut world, UiNode::new(3, 4, 10, 5));
        let mut pointer = UiPointer::default();
        assert_eq!(
            pointer.update(&world, Some((3, 4)), true),
            UiPointerResult {
                consumed: true,
                activated: None
            }
        );
        assert!(pointer.update(&world, None, true).consumed);
        assert_eq!(
            pointer.update(&world, Some((12, 8)), false).activated,
            Some(target)
        );
        assert_eq!(pointer.update(&world, Some((12, 8)), false).activated, None);
        pointer.update(&world, Some((3, 4)), true);
        assert_eq!(
            pointer.update(&world, Some((13, 4)), false),
            UiPointerResult {
                consumed: true,
                activated: None
            }
        );
        assert!(!pointer.update(&world, None, false).consumed);
        // Entering the button during a gesture that began outside cannot activate it.
        pointer.update(&world, None, true);
        assert_eq!(pointer.update(&world, Some((3, 4)), false).activated, None);
        pointer.update(&world, Some((3, 4)), true);
        assert!(pointer.cancel());
        assert!(!pointer.cancel());
        assert_eq!(pointer.update(&world, Some((3, 4)), false).activated, None);
        pointer.update(&world, Some((3, 4)), true);
        assert_eq!(
            pointer.update(&world, Some((3, 4)), false).activated,
            Some(target)
        );
    }

    #[test]
    fn disabled_hidden_and_despawned_buttons_cannot_click_through_or_activate() {
        let mut world = World::new();
        let lower = button(&mut world, UiNode::new(0, 0, 10, 10));
        let upper = button(&mut world, UiNode::new(0, 0, 10, 10));
        let mut pointer = UiPointer::default();
        world.get_mut::<UiButton>(upper).unwrap().enabled = false;
        assert!(pointer.update(&world, Some((0, 0)), true).consumed);
        world.get_mut::<UiButton>(upper).unwrap().enabled = true;
        assert_eq!(pointer.update(&world, Some((0, 0)), false).activated, None);
        pointer.update(&world, Some((0, 0)), true);
        world.get_mut::<UiNode>(upper).unwrap().visible = false;
        assert_eq!(hit_test(&world, 0, 0), Some(lower));
        assert_eq!(
            pointer.update(&world, Some((0, 0)), false),
            UiPointerResult {
                consumed: true,
                activated: None
            }
        );
        world.get_mut::<UiNode>(upper).unwrap().visible = true;
        pointer.update(&world, Some((0, 0)), true);
        world.despawn(upper);
        let replacement = button(&mut world, UiNode::new(0, 0, 10, 10));
        assert_ne!(replacement, upper);
        assert_eq!(
            pointer.update(&world, Some((0, 0)), false),
            UiPointerResult {
                consumed: true,
                activated: None
            }
        );
    }

    #[test]
    fn restoring_a_disabled_or_hidden_target_cannot_revive_an_observed_cancellation() {
        let mut world = World::new();
        let target = button(&mut world, UiNode::new(0, 0, 10, 10));
        for disable in [true, false] {
            let mut pointer = UiPointer::default();
            pointer.update(&world, Some((0, 0)), true);
            if disable {
                world.get_mut::<UiButton>(target).unwrap().enabled = false;
            } else {
                world.get_mut::<UiNode>(target).unwrap().visible = false;
            }
            assert!(pointer.update(&world, None, true).consumed);
            world.get_mut::<UiButton>(target).unwrap().enabled = true;
            world.get_mut::<UiNode>(target).unwrap().visible = true;
            assert_eq!(
                pointer.update(&world, Some((0, 0)), false),
                UiPointerResult {
                    consumed: true,
                    activated: None
                }
            );
        }
    }

    #[test]
    fn geometry_and_surface_mapping_handle_edges_and_invalid_values() {
        let node = UiNode::new(i32::MIN, -1, u32::MAX, 2);
        assert!(node.contains(i32::MIN, -1));
        assert!(node.contains(i32::MAX - 1, 0));
        assert!(!node.contains(i32::MAX, 0));
        assert!(!node.contains(0, 1));
        assert!(!UiNode::new(0, 0, 0, 1).contains(0, 0));
        assert_eq!(
            point_from_surface(400.0, 280.0, 800.0, 560.0, 160, 112),
            Some((80, 56))
        );
        assert_eq!(
            point_from_surface(799.9, 559.9, 800.0, 560.0, 160, 112),
            Some((159, 111))
        );
        for (x, y, width, height) in [
            (800.0, 0.0, 800.0, 560.0),
            (-1.0, 0.0, 800.0, 560.0),
            (0.0, 560.0, 800.0, 560.0),
            (f64::NAN, 0.0, 800.0, 560.0),
            (0.0, 0.0, f64::INFINITY, 560.0),
            (0.0, 0.0, 0.0, 560.0),
        ] {
            assert_eq!(point_from_surface(x, y, width, height, 160, 112), None);
        }
        assert_eq!(point_from_surface(0.0, 0.0, 800.0, 560.0, 0, 112), None);
    }

    #[test]
    fn inspector_exposes_ui_entity_data_as_read_only_fields() {
        let mut app = App::new();
        let entity = app.world_mut().spawn_with((
            Name::new("restart"),
            UiNode::new(4, 10, 35, 5),
            UiText::new("RESTART"),
            UiButton::default(),
        ));
        let mut config = InspectionConfig::controlled("ui-test", ".");
        config.mutation_enabled = true;
        let mut inspector = Inspector::new(config);
        register_ui_inspection(&mut inspector).unwrap();
        let id = EntityId {
            index: entity.index(),
            generation: entity.generation(),
        };
        let response = inspector.handle(
            &mut app,
            &RequestEnvelope::new("read", Request::Entity { entity: id }),
        );
        let ResponseOutcome::Success {
            response: Response::Entity(details),
        } = response.outcome
        else {
            panic!("entity inspection failed");
        };
        assert_eq!(
            details.components[std::any::type_name::<UiText>()]["text"],
            "RESTART"
        );
        assert_eq!(details.components[std::any::type_name::<UiNode>()]["x"], 4);
        assert_eq!(
            details.components[std::any::type_name::<UiButton>()]["enabled"],
            true
        );
        let response = inspector.handle(
            &mut app,
            &RequestEnvelope::new(
                "write",
                Request::SetField {
                    entity: id,
                    component: std::any::type_name::<UiText>().into(),
                    field: "text".into(),
                    value: "CHANGED".into(),
                },
            ),
        );
        assert!(
            matches!(response.outcome, ResponseOutcome::Failure { error } if error.code == ErrorCode::ReadOnly)
        );
        assert_eq!(app.world().get::<UiText>(entity).unwrap().text, "RESTART");
    }
}

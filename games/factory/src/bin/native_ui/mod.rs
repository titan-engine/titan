//! Host-owned entity UI. Rendering reads a snapshot and never ticks the game.
use serde_json::Value;
use std::collections::BTreeMap;
use titan::render::{Color, Image, ImageAssets, ImageId, RenderFrame, SpriteDraw};
use titan::ui::{BitmapFont, UiButton, UiFocus, UiNode, UiText, append_ui, hit_test};
use titan::{App, Entity, World};
use titan_factory::game;
pub const WIDTH: u32 = 800;
pub const HEIGHT: u32 = 560;
pub const WORLD_X: f64 = 12.;
pub const WORLD_Y: f64 = 92.;
pub struct Interface {
    world: World,
    assets: ImageAssets,
    image: Option<ImageId>,
    buttons: Vec<(Entity, &'static str)>,
    focus: UiFocus,
    backgrounds: BTreeMap<u32, ImageId>,
}
impl Interface {
    pub fn new() -> Self {
        let mut assets = ImageAssets::new();
        let tiny = BitmapFont::tiny(&mut assets);
        let mut glyphs = BTreeMap::new();
        for c in ' '..='Z' {
            if let Some(id) = tiny.glyph(c) {
                let source = assets.get(id).unwrap().clone();
                let image = Image::from_fn(6, 10, |x, y| {
                    let i = ((y / 2 * 3 + x / 2) * 4) as usize;
                    let p = &source.pixels()[i..i + 4];
                    Color::rgba(p[0], p[1], p[2], p[3])
                })
                .unwrap();
                glyphs.insert(c, assets.insert(image));
            }
        }
        for (c, rows) in [
            (':', [0, 2, 0, 2, 0]),
            (',', [0, 0, 0, 2, 4]),
            ('-', [0, 0, 7, 0, 0]),
            ('_', [0, 0, 0, 0, 7]),
            ('(', [1, 2, 2, 2, 1]),
            (')', [4, 2, 2, 2, 4]),
            (';', [0, 2, 0, 2, 4]),
            ('>', [4, 2, 1, 2, 4]),
            ('<', [1, 2, 4, 2, 1]),
            ('=', [0, 7, 0, 7, 0]),
            ('+', [0, 2, 7, 2, 0]),
            ('\'', [2, 2, 0, 0, 0]),
            ('!', [2, 2, 2, 0, 2]),
            ('?', [7, 1, 2, 0, 2]),
        ] {
            let image = Image::from_fn(6, 10, |x, y| {
                if rows[(y / 2) as usize] & (1 << (2 - x / 2)) != 0 {
                    Color::WHITE
                } else {
                    Color::TRANSPARENT
                }
            })
            .unwrap();
            glyphs.insert(c, assets.insert(image));
        }
        let mut world = World::new();
        world.insert_resource(BitmapFont::new(glyphs, 8, 14));
        let backgrounds = [80, 88, 104, 112, 120]
            .into_iter()
            .map(|w| {
                (
                    w,
                    assets.insert(Image::from_fn(w, 22, |_, _| Color::WHITE).unwrap()),
                )
            })
            .collect();
        Self {
            focus: UiFocus::default(),
            backgrounds,
            world,
            assets,
            image: None,
            buttons: vec![],
        }
    }
    pub fn hit(&self, x: f64, y: f64) -> Option<&'static str> {
        let entity = hit_test(&self.world, x as i32, y as i32)?;
        self.buttons
            .iter()
            .find(|(e, _)| *e == entity)
            .map(|(_, action)| *action)
    }
    pub fn focus_next(&mut self) {
        let scope = self.buttons.iter().map(|(e, _)| *e).collect::<Vec<_>>();
        self.focus.navigate(&self.world, &scope, false);
    }
    pub fn focused_action(&self) -> Option<&'static str> {
        self.buttons
            .iter()
            .find(|(e, _)| Some(*e) == self.focus.focused())
            .map(|(_, a)| *a)
    }
    fn text(&mut self, x: i32, y: i32, w: u32, h: u32, text: impl Into<String>, color: Color) {
        self.world.spawn_with((
            UiNode::new(x, y, w, h),
            UiText::new(text.into().to_uppercase().replace('→', "TO"))
                .with_color(color)
                .with_wrap(),
        ));
    }
    pub fn frame(
        &mut self,
        app: &App,
        paused: bool,
        tool: &str,
        feedback: &str,
    ) -> Result<RenderFrame, String> {
        let focused_action = self.focused_action();
        let entities: Vec<_> = self
            .world
            .iter2::<UiNode, UiText>()
            .map(|(e, _, _)| e)
            .collect();
        for e in entities {
            self.world.despawn(e);
        }
        self.buttons.clear();
        if let Some(id) = self.image.take() {
            self.assets.remove(id);
        }
        let id = self
            .assets
            .insert(game::render_image(app.world()).map_err(|e| format!("{e:?}"))?);
        self.image = Some(id);
        let mut frame = RenderFrame::new(WIDTH, HEIGHT, Color::rgb(19, 28, 39));
        frame.push(SpriteDraw::new(id, WORLD_X as i32, WORLD_Y as i32));
        let state = game::interface(app);
        let white = Color::rgb(222, 234, 241);
        let gold = Color::rgb(251, 204, 99);
        let cyan = Color::rgb(105, 215, 226);
        self.text(
            12,
            10,
            780,
            18,
            format!(
                "FACTORY / PLATES {} / 10 / {} / TICK {}",
                state["delivered"],
                if state["outcome"] == "Complete" {
                    "COMPLETE"
                } else if paused {
                    "PAUSED"
                } else {
                    "RUNNING"
                },
                state["tick"]
            ),
            gold,
        );
        let choices = [
            ("conveyor", "1 BELT", 12, 40, 104),
            ("extractor", "2 EXTRACT", 124, 40, 112),
            ("processor", "3 PROCESS", 244, 40, 112),
            ("inspect", "4 INSPECT", 364, 40, 112),
            ("rotate", "5 ROTATE", 484, 40, 104),
            ("remove", "6 REMOVE", 596, 40, 104),
            ("facing", "Q FACE", 708, 40, 80),
            ("pause", if paused { "RESUME" } else { "PAUSE" }, 12, 66, 88),
            ("step", "STEP .", 116, 66, 88),
            ("restart", "RESTART R", 220, 66, 120),
        ];
        for (action, label, x, y, w) in choices {
            let selected = action == tool;
            frame.push(
                SpriteDraw::new(self.backgrounds[&w], x, y).with_tint(if selected {
                    Color::rgb(98, 71, 31)
                } else if focused_action == Some(action) {
                    Color::rgb(40, 98, 112)
                } else {
                    Color::rgb(29, 54, 68)
                }),
            );
            let e = self.world.spawn_with((
                UiNode::new(x, y, w, 22),
                UiText::new(label).with_color(if selected { gold } else { cyan }),
                UiButton::default(),
            ));
            self.buttons.push((e, action));
            if focused_action == Some(action) {
                let scope = self.buttons.iter().map(|(e, _)| *e).collect::<Vec<_>>();
                self.focus.set(&self.world, &scope, e);
            }
        }
        self.text(
            412,
            70,
            380,
            18,
            format!(
                "TOOL {} / FACING {}",
                tool,
                state["selection"]["facing"].as_str().unwrap_or("E")
            ),
            gold,
        );
        let selected = &state["inspected"];
        let structure = if selected["structure"].is_object() {
            &selected["structure"]
        } else {
            selected
        };
        let mut lines = String::new();
        if selected.is_null() {
            lines.push_str("SELECT A TILE\nRight click a machine to pin its live details. Or choose Inspect and click.\n\n");
        } else {
            lines.push_str(&format!("TILE {} / {}\n", selected["x"], selected["y"]));
            if structure["kind"].is_string() {
                lines.push_str(&format!(
                    "{} / FACING {}\n",
                    s(&structure["kind"]),
                    s(&structure["facing"])
                ));
                let inputs = structure["inputs"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" / ")
                    })
                    .unwrap_or_default();
                lines.push_str(&format!(
                    "INPUT {} / OUTPUT {}\n",
                    if inputs.is_empty() { "NONE" } else { &inputs },
                    structure["output"].as_str().unwrap_or("NONE")
                ));
                for field in ["label", "detail", "remedy"] {
                    add(&mut lines, &structure["explanation"][field]);
                }
                if let Some(inventory) = structure["inventory"].as_array() {
                    for slot in inventory {
                        lines.push_str(&format!(
                            "{} / {} {} / {}\n",
                            s(&slot["slot"]),
                            slot["count"],
                            slot["item"].as_str().unwrap_or("EMPTY"),
                            slot["capacity"]
                        ));
                    }
                }
                add(&mut lines, &structure["recipe"]["label"]);
                if structure["recipe"].is_object() {
                    lines.push_str(&format!(
                        "PROGRESS {} / {} TICKS\n",
                        structure["recipe"]["elapsed"], structure["recipe"]["total"]
                    ));
                }
                lines.push_str("\nOUTPUT CONNECTION\n");
                for field in ["label", "detail", "remedy"] {
                    add(&mut lines, &structure["connection"][field]);
                }
            } else {
                lines.push_str("Empty tile. Choose a build tool.\n");
            }
        }
        self.text(412, 98, 380, 364, lines, white);
        let preview = &state["preview"];
        let mut preview_text = String::from("POINT AT THE GRID TO PREVIEW\n");
        if preview.is_object() {
            preview_text = format!("{}\n{}", s(&preview["label"]), s(&preview["detail"]));
        }
        self.text(
            12,
            360,
            384,
            70,
            preview_text,
            if preview["valid"] == false {
                gold
            } else {
                cyan
            },
        );
        self.text(12,436,384,56,"BUILD: EXTRACTOR ON ORE (1,3).\nPROCESSOR EAST; BELTS FILL GAPS TO (10,3).\nQ FACING / WASD PAN / WHEEL ZOOM.\nSPACE PAUSE / TAB THEN ENTER: UI.",white);
        self.text(12, 502, 384, 56, s(&state["ui"]["legend"]), cyan);
        self.text(
            412,
            468,
            380,
            28,
            format!(
                "DISCARDED {} ORE / {} PLATES",
                state["discarded_ore"], state["discarded_plate"]
            ),
            gold,
        );
        self.text(412, 500, 380, 56, feedback, cyan);
        append_ui(&self.world, &mut frame);
        Ok(frame)
    }
    pub fn assets(&self) -> &ImageAssets {
        &self.assets
    }
}
fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn add(lines: &mut String, v: &Value) {
    if let Some(s) = v.as_str() {
        lines.push_str(s);
        lines.push('\n');
    }
}
pub fn surface(x: f64, y: f64, w: u32, h: u32) -> Option<(f64, f64)> {
    if w == 0
        || h == 0
        || !x.is_finite()
        || !y.is_finite()
        || x < 0.
        || y < 0.
        || x >= w as f64
        || y >= h as f64
    {
        return None;
    }
    Some((x * WIDTH as f64 / w as f64, y * HEIGHT as f64 / h as f64))
}
pub fn world_point(x: f64, y: f64) -> Option<(f64, f64)> {
    let x = x - WORLD_X;
    let y = y - WORLD_Y;
    (x >= 0. && y >= 0. && x < game::WIDTH as f64 && y < game::HEIGHT as f64).then_some((x, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ui_text_is_complete_and_reads_do_not_tick() {
        let mut ui = Interface::new();
        let mut apps = vec![];
        for fixture in [
            "disconnected",
            "cycle_partial",
            "contention",
            "ports",
            "cycle_full",
        ] {
            apps.push(game::build_transport_fixture(fixture).unwrap());
        }
        for fixture in ["isolated_extractor", "processor_input", "processor_blocked"] {
            apps.push(game::build_production_fixture(fixture).unwrap());
        }
        for mut app in apps {
            for y in 0..8 {
                for x in 0..12 {
                    game::player_command(
                        &mut app,
                        &serde_json::json!({"op":"inspect","x":x,"y":y}).to_string(),
                    )
                    .unwrap();
                    let action = ["place", "rotate", "remove", "inspect"][(x + y) as usize % 4];
                    game::set_preview_action(&mut app, action).unwrap();
                    game::pointer(
                        &mut app,
                        x as f64 * 32. + 16.,
                        y as f64 * 32. + 16.,
                        "hover",
                    )
                    .unwrap();
                    let before = game::status(&app);
                    ui.frame(&app, true, "inspect", "").unwrap();
                    assert_eq!(before, game::status(&app));
                    let font = ui.world.resource::<BitmapFont>().unwrap();
                    for (_, node, text) in ui.world.iter2::<UiNode, UiText>() {
                        assert!(
                            !font
                                .measure_wrapped(&text.text, node.width, node.height)
                                .truncated,
                            "truncated {:?}",
                            text.text
                        );
                        assert!(
                            text.text
                                .chars()
                                .all(|c| c.is_whitespace() || font.glyph(c).is_some()),
                            "missing glyph {:?}",
                            text.text
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn grid_mapping_excludes_palette_and_handles_resize() {
        assert_eq!(world_point(12., 40.), None);
        assert_eq!(world_point(12., 92.), Some((0., 0.)));
        assert_eq!(world_point(396., 92.), None);
        for (w, h) in [(800, 560), (1200, 840), (2000, 1400)] {
            let (x, y) = surface(204. * w as f64 / 800., 220. * h as f64 / 560., w, h).unwrap();
            assert_eq!(world_point(x, y), Some((192., 128.)));
        }
        assert_eq!(surface(f64::NAN, 0., 800, 560), None);
    }
}

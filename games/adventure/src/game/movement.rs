//! Game-local integer swept AABB movement; characters never enter the solids list.
use super::Position;
use serde::Serialize;
use titan::Component;

pub const HALF: i32 = 200;
pub const HEIGHT: i32 = 900;
pub const GRAVITY: i32 = 10;
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Solid {
    pub name: &'static str,
    pub min: Position,
    pub max: Position,
}
const fn solid(name: &'static str, min: (i32, i32, i32), max: (i32, i32, i32)) -> Solid {
    Solid {
        name,
        min: Position {
            x: min.0,
            y: min.1,
            z: min.2,
        },
        max: Position {
            x: max.0,
            y: max.1,
            z: max.2,
        },
    }
}
pub const SOLIDS: [Solid; 9] = [
    solid("floor", (0, -1000, 0), (12000, 0, 8000)),
    solid("wall-west", (-200, 0, -200), (0, 4000, 8200)),
    solid("wall-east", (12000, 0, -200), (12200, 4000, 8200)),
    solid("wall-north", (0, 0, -200), (12000, 4000, 0)),
    solid("wall-south", (0, 0, 8000), (12000, 4000, 8200)),
    solid("teaching-ledge", (1000, 0, 1000), (3000, 1000, 3000)),
    solid("high-ledge", (4000, 0, 1000), (7000, 2000, 3000)),
    solid("practice-step", (5050, 0, 3050), (5950, 750, 3950)),
    solid("practice-ceiling", (9000, 1300, 4500), (11000, 1550, 5500)),
];
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Contacts {
    pub x: Option<&'static str>,
    pub z: Option<&'static str>,
    pub ceiling: Option<&'static str>,
    pub landed: Option<&'static str>,
}
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Movement {
    pub velocity_y: i32,
    pub grounded: bool,
    pub support: Option<&'static str>,
    pub collisions: Contacts,
}
impl Default for Movement {
    fn default() -> Self {
        Self {
            velocity_y: 0,
            grounded: true,
            support: Some("floor"),
            collisions: Contacts::default(),
        }
    }
}
fn overlap(a0: i32, a1: i32, b0: i32, b1: i32) -> bool {
    a0 < b1 && a1 > b0
}
fn footprint(p: Position, s: &Solid) -> bool {
    overlap(p.x - HALF, p.x + HALF, s.min.x, s.max.x)
        && overlap(p.z - HALF, p.z + HALF, s.min.z, s.max.z)
}
fn support(p: Position, solids: &[Solid]) -> Option<&'static str> {
    solids
        .iter()
        .find(|s| p.y == s.max.y && footprint(p, s))
        .map(|s| s.name)
}
pub fn advance(
    p: &mut Position,
    m: &mut Movement,
    dx: i32,
    dz: i32,
    jump: bool,
    jump_speed: i32,
    solids: &[Solid],
) {
    m.collisions = Contacts::default();
    // Sample launch support before horizontal movement: no coyote or buffered jump.
    m.support = support(*p, solids);
    m.grounded = m.support.is_some() && m.velocity_y == 0;
    if jump && m.grounded {
        m.velocity_y = jump_speed;
        m.grounded = false;
        m.support = None;
    }
    for (x_axis, delta) in [(true, dx), (false, dz)] {
        let old = if x_axis { p.x } else { p.z };
        let mut end = old + delta;
        let mut contact = None;
        for s in solids {
            let other_overlap = if x_axis {
                overlap(p.z - HALF, p.z + HALF, s.min.z, s.max.z)
            } else {
                overlap(p.x - HALF, p.x + HALF, s.min.x, s.max.x)
            };
            if !other_overlap || !overlap(p.y, p.y + HEIGHT, s.min.y, s.max.y) {
                continue;
            }
            let (low, high) = if x_axis {
                (s.min.x, s.max.x)
            } else {
                (s.min.z, s.max.z)
            };
            if delta > 0 && old + HALF <= low && end + HALF >= low {
                end = low - HALF;
                contact = Some(s.name);
            }
            if delta < 0 && old - HALF >= high && end - HALF <= high {
                end = high + HALF;
                contact = Some(s.name);
            }
        }
        if x_axis {
            p.x = end;
            m.collisions.x = contact;
        } else {
            p.z = end;
            m.collisions.z = contact;
        }
    }
    if m.grounded {
        m.support = support(*p, solids);
        m.grounded = m.support.is_some();
    }
    if m.grounded {
        return;
    }
    m.support = None;
    m.velocity_y -= GRAVITY;
    let old = p.y;
    let mut end = old + m.velocity_y;
    for s in solids {
        if !footprint(*p, s) {
            continue;
        }
        if m.velocity_y > 0 && old + HEIGHT <= s.min.y && end + HEIGHT >= s.min.y {
            end = s.min.y - HEIGHT;
            m.collisions.ceiling = Some(s.name);
        } else if m.velocity_y < 0 && old >= s.max.y && end <= s.max.y {
            end = s.max.y;
            m.collisions.landed = Some(s.name);
        }
    }
    p.y = end;
    if m.collisions.ceiling.is_some() {
        m.velocity_y = 0;
    }
    if let Some(name) = m.collisions.landed {
        m.velocity_y = 0;
        m.grounded = true;
        m.support = Some(name);
    }
}

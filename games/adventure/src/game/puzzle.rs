//! Room 1's bounded plate, doorway and exit rules, sampled after movement.
use super::{
    Position, character_name,
    movement::{self, HALF, HEIGHT, Movement, Solid},
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Rect {
    pub min_x: i32,
    pub max_x: i32,
    pub min_z: i32,
    pub max_z: i32,
    pub y: i32,
}
pub const PLATES: [Rect; 2] = [
    Rect {
        min_x: 1700,
        max_x: 2300,
        min_z: 1700,
        max_z: 2300,
        y: 1000,
    },
    Rect {
        min_x: 9700,
        max_x: 10300,
        min_z: 4700,
        max_z: 5300,
        y: 0,
    },
];
pub fn plates(room: u8) -> [Rect; 2] {
    let mut plates = PLATES;
    if room == 2 {
        plates[0] = Rect {
            min_x: 5200,
            max_x: 5800,
            min_z: 1700,
            max_z: 2300,
            y: 2000,
        };
    }
    plates
}
pub const EXIT: Rect = Rect {
    min_x: 10000,
    max_x: 12000,
    min_z: 1000,
    max_z: 3000,
    y: 0,
};
pub const DOOR: Solid = movement::solid("door", (7000, 0, 4000), (8000, 4000, 6000));
#[derive(Clone, Debug, Serialize)]
pub struct PlateState {
    pub id: &'static str,
    pub pressed: bool,
    pub occupants: Vec<&'static str>,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct DoorState {
    pub state: &'static str,
    pub open: bool,
}
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ExitState {
    pub jumper: bool,
    pub strong: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct PuzzleState {
    pub plates: [PlateState; 2],
    pub door: DoorState,
    pub exit: ExitState,
    pub complete: bool,
}
impl Default for PuzzleState {
    fn default() -> Self {
        Self {
            plates: ["A", "B"].map(|id| PlateState {
                id,
                pressed: false,
                occupants: Vec::new(),
            }),
            door: DoorState {
                state: "closed",
                open: false,
            },
            exit: ExitState::default(),
            complete: false,
        }
    }
}
impl PuzzleState {
    pub fn sample(&mut self, bodies: [(Position, Movement); 2]) {
        self.sample_room(bodies, 1);
    }
    pub fn sample_room(&mut self, bodies: [(Position, Movement); 2], room: u8) {
        if self.complete {
            return;
        }
        for (plate, rect) in self.plates.iter_mut().zip(plates(room)) {
            plate.occupants = bodies
                .iter()
                .enumerate()
                .filter_map(|(index, (p, m))| {
                    (m.grounded
                        && p.y == rect.y
                        && p.x >= rect.min_x
                        && p.x <= rect.max_x
                        && p.z >= rect.min_z
                        && p.z <= rect.max_z)
                        .then_some(character_name(index))
                })
                .collect();
            plate.pressed = !plate.occupants.is_empty();
        }
        let requested = self.plates.iter().any(|p| p.pressed);
        let obstructed = bodies.iter().any(|(p, _)| {
            p.x + HALF > DOOR.min.x
                && p.x - HALF < DOOR.max.x
                && p.z + HALF > DOOR.min.z
                && p.z - HALF < DOOR.max.z
                && p.y + HEIGHT > DOOR.min.y
                && p.y < DOOR.max.y
        });
        self.door = DoorState {
            state: if requested {
                "open_plate"
            } else if obstructed {
                "open_obstructed"
            } else {
                "closed"
            },
            open: requested || obstructed,
        };
        let at_exit = bodies.map(|(p, m)| {
            m.grounded
                && p.y == EXIT.y
                && p.x - HALF >= EXIT.min_x
                && p.x + HALF <= EXIT.max_x
                && p.z - HALF >= EXIT.min_z
                && p.z + HALF <= EXIT.max_z
        });
        self.exit = ExitState {
            jumper: at_exit[0],
            strong: at_exit[1],
        };
        self.complete = at_exit[0] && at_exit[1];
    }
}

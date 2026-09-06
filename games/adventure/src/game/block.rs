//! Atomic room-local rail steps. Collision and support always use the settled socket.
use super::{
    Position,
    movement::{self, HALF, HEIGHT, Movement, Solid},
};
use serde::Serialize;

pub const SOCKET_Z: [i32; 3] = [5500, 4500, 3500];
#[derive(Clone, Debug, Default, Serialize)]
pub struct BlockState {
    pub socket: usize,
    pub moves: u32,
    pub last_rejection: Option<&'static str>,
}
impl BlockState {
    pub fn solid(&self) -> Solid {
        let z = SOCKET_Z[self.socket];
        movement::solid("heavy-block", (5050, 0, z - 450), (5950, 750, z + 450))
    }
    /// Priority is part of the replay contract. Failure never mutates the socket.
    pub fn push(
        &mut self,
        active: usize,
        directions: (i32, i32, usize),
        jump: bool,
        bodies: [(Position, Movement); 2],
        solids: &[Solid],
    ) -> bool {
        let (p, m) = bodies[active];
        let (dx, dz, count) = directions;
        let z = SOCKET_Z[self.socket];
        let reason = if active != 1 {
            Some("wrong_character")
        } else if !m.grounded || m.velocity_y != 0 || p.y != 0 || jump {
            Some("not_grounded")
        } else if count != 1 || dx != 0 || dz == 0 {
            Some("invalid_direction")
        } else if (p.x - 5500).pow(2) + (p.z - (z - dz * 1000)).pow(2) > 100 * 100 {
            Some("invalid_stance")
        } else if (dz < 0 && self.socket == 2) || (dz > 0 && self.socket == 0) {
            Some("rail_end")
        } else if bodies.iter().any(|(_, m)| m.support == Some("heavy-block")) {
            Some("block_occupied")
        } else {
            let old = self.solid();
            let sweep = movement::solid(
                "push-sweep",
                (old.min.x, 0, old.min.z.min(old.min.z + dz * 1000)),
                (old.max.x, 750, old.max.z.max(old.max.z + dz * 1000)),
            );
            let bodies_block = bodies.iter().any(|(p, _)| {
                overlap(
                    &sweep,
                    &movement::solid(
                        "body",
                        (p.x - HALF, p.y, p.z - HALF),
                        (p.x + HALF, p.y + HEIGHT, p.z + HALF),
                    ),
                )
            });
            if bodies_block
                || solids
                    .iter()
                    .any(|s| s.name != "floor" && s.name != "heavy-block" && overlap(&sweep, s))
            {
                Some("path_obstructed")
            } else {
                None
            }
        };
        self.last_rejection = reason;
        if reason.is_some() {
            return false;
        }
        self.socket = (self.socket as i32 - dz) as usize;
        self.moves += 1;
        true
    }
}
fn overlap(a: &Solid, b: &Solid) -> bool {
    a.min.x < b.max.x
        && a.max.x > b.min.x
        && a.min.y < b.max.y
        && a.max.y > b.min.y
        && a.min.z < b.max.z
        && a.max.z > b.min.z
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bodies(z: i32) -> [(Position, Movement); 2] {
        [
            (
                Position {
                    x: 1500,
                    y: 0,
                    z: 6500,
                },
                Movement::default(),
            ),
            (Position { x: 5500, y: 0, z }, Movement::default()),
        ]
    }
    #[test]
    fn rejection_priority_and_atomicity() {
        let mut block = BlockState::default();
        let mut b = bodies(6500);
        b[0].1.grounded = false;
        assert!(!block.push(0, (0, 0, 0), true, b, &[]));
        assert_eq!(block.last_rejection, Some("wrong_character"));
        assert!(!block.push(1, (0, 0, 0), true, b, &[]));
        assert_eq!(block.last_rejection, Some("not_grounded"));
        b[1].1.grounded = false;
        assert!(!block.push(1, (0, -1, 1), false, b, &[]));
        assert_eq!(block.last_rejection, Some("not_grounded"));
        b[1].1 = Movement::default();
        for direction in [(0, 0, 0), (0, 0, 2), (1, -1, 2), (1, 0, 1)] {
            assert!(!block.push(1, direction, false, b, &[]));
            assert_eq!(block.last_rejection, Some("invalid_direction"));
        }
        b[1].0.x += 101;
        assert!(!block.push(1, (0, -1, 1), false, b, &[]));
        assert_eq!(block.last_rejection, Some("invalid_stance"));
        assert_eq!((block.socket, block.moves), (0, 0));
        b = bodies(4500);
        assert!(!block.push(1, (0, 1, 1), false, b, &[]));
        assert_eq!(block.last_rejection, Some("rail_end"));
        b = bodies(6500);
        b[0].1.support = Some("heavy-block");
        let obstruction = movement::solid("obstacle", (5100, 0, 4300), (5900, 700, 4700));
        assert!(!block.push(1, (0, -1, 1), false, b, &[obstruction]));
        assert_eq!(block.last_rejection, Some("block_occupied"));
        b[0].1 = Movement::default();
        assert!(!block.push(1, (0, -1, 1), false, b, &[obstruction]));
        assert_eq!(block.last_rejection, Some("path_obstructed"));
        assert_eq!((block.socket, block.moves), (0, 0));
    }
    #[test]
    fn swept_body_volume_support_and_exact_contact() {
        for (x, y, z, blocked) in [
            (5500, 0, 4500, true),
            (5500, 749, 4500, true),
            (5500, 750, 4500, false),
            (6150, 0, 4500, false),
            (6149, 0, 4500, true),
            (5500, 0, 3850, false),
            (5500, 0, 3851, true),
        ] {
            let mut block = BlockState::default();
            let mut b = bodies(6500);
            b[0].0 = Position { x, y, z };
            b[0].1.support = None;
            b[0].1.grounded = false;
            assert_eq!(
                block.push(1, (0, -1, 1), false, b, &[]),
                !blocked,
                "{x} {y} {z}"
            );
            assert_eq!(block.socket, usize::from(!blocked));
        }
        // A supported character is occupied even though its body lies above the sweep.
        let mut block = BlockState::default();
        let mut b = bodies(6500);
        b[0].0 = Position {
            x: 5500,
            y: 750,
            z: 5500,
        };
        b[0].1.support = Some("heavy-block");
        assert!(!block.push(1, (0, -1, 1), false, b, &[]));
        assert_eq!(block.last_rejection, Some("block_occupied"));
    }
    #[test]
    fn adjacent_steps_reversal_endpoints_and_floor_contact() {
        let mut block = BlockState::default();
        let solids = super::super::room_solids(2);
        let mut b = bodies(6500);
        b[1].0.x += 100; // Inclusive circular stance tolerance.
        assert!(block.push(1, (0, -1, 1), false, b, &solids));
        assert_eq!((block.socket, block.moves), (1, 1));
        assert!(block.push(1, (0, 1, 1), false, bodies(3500), &solids));
        assert_eq!(block.socket, 0);
        assert!(block.push(1, (0, -1, 1), false, bodies(6500), &solids));
        assert!(block.push(1, (0, -1, 1), false, bodies(5500), &solids));
        assert_eq!(block.socket, 2);
        assert!(!block.push(1, (0, -1, 1), false, bodies(4500), &solids));
        assert_eq!(block.last_rejection, Some("rail_end"));
        assert_eq!(block.solid().max.y, 750);
    }
}

//! Device presentation comes from the same sampled puzzle state as inspection.
use super::*;

#[derive(Component)]
pub(super) struct PuzzleHud(u8);

pub(super) fn setup(world: &mut World) {
    for (index, y) in [18, 28, 165, 155].into_iter().enumerate() {
        world.spawn_with((
            Name::new(format!("ui/puzzle-{index}")),
            PuzzleHud(index as u8),
            UiNode::new(8, y, 304, 5),
            UiText::new("").with_color(Color::rgb(240, 235, 205)),
        ));
    }
}

pub(super) fn sync(world: &mut World) {
    let session = world.resource::<Session>().unwrap();
    let puzzle = &session.puzzle;
    let pressed = |i: usize| {
        if puzzle.plates[i].pressed {
            "HELD"
        } else {
            "EMPTY"
        }
    };
    let door = match puzzle.door.state {
        "open_plate" => "OPEN: PLATE",
        "open_obstructed" => "OPEN: BODY IN DOOR",
        _ => "CLOSED",
    };
    let text = [
        format!("A || {}  B || {}  DOOR || {}", pressed(0), pressed(1), door),
        format!(
            "EXIT: JUMPER {}  STRONG {}",
            if puzzle.exit.jumper { "IN" } else { "OUT" },
            if puzzle.exit.strong { "IN" } else { "OUT" }
        ),
        if puzzle.complete {
            format!("ROOM {} COMPLETE!  [R] RESTART ROOM", session.room)
        } else {
            if session.room == 2 {
                "[E]+UP/DOWN PUSH  BLOCK -> A -> B -> EXIT".into()
            } else {
                "HOLD A, CROSS TO B, BRING BOTH TO EXIT".into()
            }
        },
        if session.room == 2 {
            let feedback = match session.block.last_rejection {
                Some("wrong_character") => "STRONG ONLY",
                Some("not_grounded") => "STAND ON FLOOR - NO JUMP",
                Some("invalid_direction") => "ONE RAIL DIRECTION REQUIRED",
                Some("invalid_stance") => "STAND 1M BEHIND BLOCK",
                Some("rail_end") => "RAIL END",
                Some("block_occupied") => "BLOCK OCCUPIED",
                Some("path_obstructed") => "PATH OBSTRUCTED",
                _ => "READY",
            };
            format!("K SOCKET {} / 3: {}", session.block.socket + 1, feedback)
        } else {
            String::new()
        },
    ];
    let ids: Vec<_> = world.iter::<PuzzleHud>().map(|(id, h)| (id, h.0)).collect();
    for (id, index) in ids {
        world.get_mut::<UiText>(id).unwrap().text = text[index as usize].clone();
    }
}

pub(super) fn append(world: &World, draws: &mut Vec<Draw3d>) {
    let session = world.resource::<Session>().unwrap();
    let puzzle = &session.puzzle;
    let cube = world.resource::<Markers>().unwrap().cube;
    let mut order = 1u64 << 63;
    let mut block = |x, y, z, sx, sy, sz, color| {
        draws.push(Draw3d {
            mesh: cube,
            transform: Transform3d::new(
                Vec3::new(x, y, z),
                Quaternion::IDENTITY,
                Vec3::new(sx, sy, sz),
            )
            .unwrap(),
            color,
            order,
        });
        order += 1;
    };
    if session.room == 2 {
        for z in block::SOCKET_Z {
            let z = z as f32 / 1000.;
            for x in [5.04, 5.96] {
                block(
                    x,
                    0.018,
                    z,
                    0.04,
                    0.025,
                    0.92,
                    BaseColor::rgb(185, 170, 110),
                );
            }
            for offset in [-0.46, 0.46] {
                block(
                    5.5,
                    0.018,
                    z + offset,
                    0.92,
                    0.025,
                    0.04,
                    BaseColor::rgb(185, 170, 110),
                );
            }
        }
        let z = block::SOCKET_Z[session.block.socket] as f32 / 1000.;
        block(5.5, 0.375, z, 0.9, 0.75, 0.9, BaseColor::rgb(165, 115, 65));
        for x in [5.25, 5.75] {
            block(x, 0.76, z, 0.07, 0.025, 0.55, BaseColor::rgb(250, 222, 150));
        }
    }
    let link = BaseColor::rgb(255, 237, 175);
    for (plate, rect) in puzzle.plates.iter().zip(puzzle::plates(session.room)) {
        let x = (rect.min_x + rect.max_x) as f32 / 2000.;
        let z = (rect.min_z + rect.max_z) as f32 / 2000.;
        let y = rect.y as f32 / 1000.;
        let thickness = if plate.pressed { 0.025 } else { 0.07 };
        block(
            x,
            y + thickness / 2.,
            z,
            0.6,
            thickness,
            0.6,
            if plate.pressed {
                BaseColor::rgb(80, 220, 150)
            } else {
                BaseColor::rgb(200, 145, 65)
            },
        );
        for offset in [-0.13, 0.13] {
            block(x + offset, y + thickness + 0.01, z, 0.065, 0.015, 0.4, link);
        }
    }
    // Two linked stripes identify D when its collision volume is absent. The
    // closed gate and partitions use a cutaway; inspection retains the 4m top.
    let door_color = if puzzle.door.state == "open_obstructed" {
        BaseColor::rgb(255, 190, 80)
    } else if puzzle.door.open {
        BaseColor::rgb(80, 220, 150)
    } else {
        BaseColor::rgb(190, 95, 75)
    };
    if !puzzle.door.open {
        block(7.5, 0.6, 5., 1., 1.2, 2., door_color);
    }
    let y = if puzzle.door.open { 0.04 } else { 1.22 };
    for x in [7.25, 7.75] {
        block(x, y, 5., 0.08, 0.03, 1.7, link);
    }
    for z in [4.05, 5.95] {
        block(7.5, 0.045, z, 1., 0.07, 0.1, door_color);
    }
    // Exit outline remains legible under either character; two small markers
    // echo the separate HUD occupancy indicators.
    let color = BaseColor::rgb(100, 200, 160);
    for (x, z, sx, sz) in [
        (10.05, 2., 0.1, 2.),
        (11.95, 2., 0.1, 2.),
        (11., 1.05, 1.8, 0.1),
        (11., 2.95, 1.8, 0.1),
    ] {
        block(x, 0.025, z, sx, 0.04, sz, color);
    }
    for (x, inside) in [(10.65, puzzle.exit.jumper), (11.35, puzzle.exit.strong)] {
        block(
            x,
            0.035,
            1.4,
            0.35,
            0.05,
            0.2,
            if inside {
                link
            } else {
                BaseColor::rgb(60, 110, 105)
            },
        );
    }
}

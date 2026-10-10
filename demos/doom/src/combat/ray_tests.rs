//! Grid traversal regressions, including a cell-query budget rather than timing.

use super::*;

// Deliberately slow, independent ray/AABB reference kept only in tests.
fn slab_distance(level: &Level, origin: Vec2, direction: Vec2, range: f32) -> f32 {
    let mut nearest = range;
    for z in 0..level.height() {
        for x in 0..level.width() {
            if !level.is_wall(x as i32, z as i32) {
                continue;
            }
            let min = Vec2::new(x as f32, z as f32);
            let max = min + Vec2::ONE;
            let mut enter: f32 = 0.0;
            let mut leave = range;
            for axis in 0..2 {
                if direction[axis] == 0.0 {
                    if origin[axis] < min[axis] || origin[axis] > max[axis] {
                        leave = -1.0;
                    }
                } else {
                    let a = (min[axis] - origin[axis]) / direction[axis];
                    let b = (max[axis] - origin[axis]) / direction[axis];
                    enter = enter.max(a.min(b));
                    leave = leave.min(a.max(b));
                }
            }
            if enter <= leave {
                nearest = nearest.min(enter);
            }
        }
    }
    nearest
}

fn patterned_level() -> Level {
    let rows: Vec<String> = (0..16)
        .map(|z| {
            (0..16)
                .map(|x| {
                    if x == 0
                        || z == 0
                        || x == 15
                        || z == 15
                        || ((x, z) != (1, 1) && (x * 13 + z * 7) % 11 == 0)
                    {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect()
        })
        .collect();
    Level::parse(&format!(
        "(rows: {rows:?}, objects: [(id: \"spawn\", kind: Spawn, position: (1.5, 1.5), yaw: 0.0)])"
    ))
    .unwrap()
}

#[test]
fn grid_traversal_agrees_with_independent_slabs_for_axes_corners_and_pitched_rays() {
    let level = patterned_level();
    for z in 1..15 {
        for x in 1..15 {
            for offset in [
                Vec2::splat(0.5),
                Vec2::ZERO,
                Vec2::new(0.0, 0.5),
                Vec2::new(0.5, 0.0),
            ] {
                let origin = Vec2::new(x as f32, z as f32) + offset;
                for direction in [
                    Vec2::ZERO,
                    Vec2::X,
                    -Vec2::X,
                    Vec2::Y,
                    -Vec2::Y,
                    Vec2::ONE,
                    -Vec2::ONE,
                    Vec2::new(1.0, -1.0),
                    Vec2::new(-1.0, 1.0),
                    Vec2::new(0.13, 0.81),
                    Vec2::new(-0.77, 0.21),
                    Vec2::new(0.001, -0.01),
                ] {
                    let expected = slab_distance(&level, origin, direction, SHOT_RANGE);
                    let actual = wall_distance(&level, origin, direction, SHOT_RANGE);
                    assert!(
                        (actual - expected).abs() < 0.00001,
                        "{origin:?} + t*{direction:?}: {actual} != {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn exact_corner_crossings_check_both_side_cells_in_every_quadrant() {
    let origin = Vec2::splat(5.5);
    for x in [-1, 1] {
        for z in [-1, 1] {
            let direction = Vec2::new(x as f32, z as f32);
            for wall in [IVec2::new(5 + 2 * x, 5 + z), IVec2::new(5 + x, 5 + 2 * z)] {
                assert_eq!(
                    grid_ray_distance(origin, direction, 10.0, |cell| cell == wall),
                    1.5
                );
            }
        }
    }
}

#[test]
fn boundary_aligned_rays_check_both_sides_and_starting_contacts() {
    for (origin, direction, wall) in [
        (Vec2::new(3.0, 1.5), Vec2::Y, IVec2::new(2, 3)),
        (Vec2::new(3.0, 5.5), -Vec2::Y, IVec2::new(2, 3)),
        (Vec2::new(1.5, 3.0), Vec2::X, IVec2::new(3, 2)),
        (Vec2::new(5.5, 3.0), -Vec2::X, IVec2::new(3, 2)),
    ] {
        assert_eq!(
            grid_ray_distance(origin, direction, 10.0, |cell| cell == wall),
            1.5
        );
    }
    assert_eq!(
        grid_ray_distance(Vec2::new(3.0, 3.5), Vec2::X, 10.0, |cell| cell
            == IVec2::new(2, 3)),
        0.0
    );
    assert_eq!(
        grid_ray_distance(Vec2::new(3.5, 3.5), Vec2::X, 0.25, |cell| cell
            == IVec2::new(4, 3)),
        0.25
    );
}

#[test]
fn maximum_grid_queries_scale_with_ray_length_not_map_area_or_object_count() {
    // The maximum-sized validated map is represented by this solid-cell query.
    // Even 1024 rays (the level object limit) stay under a fixed per-ray budget;
    // no wall-clock assertion or loaded-runner assumptions are needed.
    for range in [10.0, SHOT_RANGE] {
        let mut total_queries = 0;
        for index in 0..1024 {
            let direction = Vec2::new((index % 17) as f32 - 8.0, (index % 23) as f32 - 11.0)
                .normalize_or_zero();
            let mut queries = 0;
            let distance = grid_ray_distance(Vec2::splat(64.5), direction, range, |cell| {
                queries += 1;
                cell.x <= 0 || cell.y <= 0 || cell.x >= 127 || cell.y >= 127
            });
            assert_eq!(distance, range);
            assert!(queries <= 100, "{queries} cell queries for {direction:?}");
            total_queries += queries;
        }
        assert!(total_queries <= 1024 * 100);
    }
}

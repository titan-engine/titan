//! Versioned gameplay snapshots; entity handles, assets and host time are not saved.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_SAVE_BYTES: usize = 64 * 1024;
pub const GAME_SEED: u64 = 0x0054_4954_414e;
const MAX_SHARDS: usize = 256;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Point {
    x: i32,
    y: i32,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SavedShard {
    name: String,
    position: Point,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    format_version: u32,
    game_seed: u64,
    player: Point,
    shrine: Point,
    shards: Vec<SavedShard>,
    collected_shards: usize,
    shrine_active: bool,
}
fn invalid(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidValue, message)
}
fn point(position: &Position) -> Point {
    Point {
        x: position.x,
        y: position.y,
    }
}
fn valid_point(point: &Point) -> bool {
    (0..MAP_WIDTH).contains(&point.x) && (0..MAP_HEIGHT).contains(&point.y)
}
fn topology(world: &World) -> Result<(titan::Entity, titan::Entity), ProtocolError> {
    let labels: Vec<_> = world.iter::<QuestHud>().map(|(e, _)| e).collect();
    if world.resource::<QuestState>().is_none()
        || world.resource::<Art>().is_none()
        || world.resource::<ImageAssets>().is_none()
        || world.resource::<BitmapFont>().is_none()
        || labels.len() != 1
        || world.get::<UiText>(labels[0]).is_none()
        || world.get::<UiNode>(labels[0]).is_none()
    {
        return Err(invalid("snapshot target lacks RPG resources or quest HUD"));
    }
    let players: Vec<_> = world.iter::<Player>().map(|(e, _)| e).collect();
    let shrines: Vec<_> = world.iter::<Shrine>().map(|(e, _)| e).collect();
    if players.len() != 1
        || shrines.len() != 1
        || world.get::<Position>(players[0]).is_none()
        || world.get::<Position>(shrines[0]).is_none()
    {
        return Err(invalid(
            "snapshot requires one positioned player and shrine",
        ));
    }
    Ok((players[0], shrines[0]))
}
pub fn export_world(world: &World) -> Result<Value, ProtocolError> {
    let (player, shrine) = topology(world)?;
    let quest = world.resource::<QuestState>().unwrap();
    let mut entities: Vec<_> = world.iter::<Shard>().map(|(e, _)| e).collect();
    entities.sort();
    if entities.len() > MAX_SHARDS {
        return Err(invalid("snapshot exceeds 256 remaining shards"));
    }
    let mut shards = entities
        .into_iter()
        .map(|e| {
            Ok(SavedShard {
                name: world
                    .get::<Name>(e)
                    .ok_or_else(|| invalid("shard has no name"))?
                    .as_str()
                    .to_owned(),
                position: point(
                    world
                        .get::<Position>(e)
                        .ok_or_else(|| invalid("shard has no position"))?,
                ),
            })
        })
        .collect::<Result<Vec<_>, ProtocolError>>()?;
    shards.sort_by(|a, b| {
        (&a.name, a.position.x, a.position.y).cmp(&(&b.name, b.position.x, b.position.y))
    });
    let snapshot = Snapshot {
        format_version: 1,
        game_seed: GAME_SEED,
        player: point(world.get::<Position>(player).unwrap()),
        shrine: point(world.get::<Position>(shrine).unwrap()),
        shards,
        collected_shards: quest.collected_shards,
        shrine_active: quest.shrine_active,
    };
    let value = serde_json::to_value(snapshot).map_err(|e| invalid(e.to_string()))?;
    validate(value.clone())?;
    Ok(value)
}
pub fn export(app: &App) -> Result<Value, ProtocolError> {
    export_world(app.world())
}
fn validate(value: Value) -> Result<Snapshot, ProtocolError> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|n| *n <= MAX_SAVE_BYTES)
                .ok_or_else(|| std::io::Error::other("snapshot exceeds 64 KiB"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter(0), &value).map_err(|e| invalid(e.to_string()))?;
    let snapshot: Snapshot = serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?;
    if snapshot.format_version != 1 || snapshot.game_seed != GAME_SEED {
        return Err(invalid("unsupported RPG snapshot identity"));
    }
    if !valid_point(&snapshot.player)
        || !valid_point(&snapshot.shrine)
        || snapshot.shards.len() > MAX_SHARDS
        || snapshot
            .shards
            .iter()
            .any(|s| !valid_point(&s.position) || s.name.len() > 128 || s.name.is_empty())
    {
        return Err(invalid(
            "invalid RPG snapshot coordinates, shard count or name",
        ));
    }
    if snapshot.collected_shards > 1_000_000
        || snapshot.shrine_active != (snapshot.collected_shards >= 3)
    {
        return Err(invalid("invalid RPG quest state"));
    }
    Ok(snapshot)
}
/// Validate everything before replacing gameplay entities. Host time is preserved.
pub fn load(app: &mut App, value: Value) -> Result<(), ProtocolError> {
    let snapshot = validate(value)?;
    let (player, shrine) = topology(app.world())?;
    let world = app.world_mut();
    *world.get_mut::<Position>(player).unwrap() = Position {
        x: snapshot.player.x,
        y: snapshot.player.y,
    };
    *world.get_mut::<Position>(shrine).unwrap() = Position {
        x: snapshot.shrine.x,
        y: snapshot.shrine.y,
    };
    let shards: Vec<_> = world.iter::<Shard>().map(|(e, _)| e).collect();
    for entity in shards {
        world.despawn(entity);
    }
    for shard in snapshot.shards {
        spawn_at(
            world,
            Position {
                x: shard.position.x,
                y: shard.position.y,
            },
            Shard,
            &shard.name,
        );
    }
    if snapshot.shrine_active {
        world.insert(shrine, ActiveShrine).expect("existing shrine");
    } else {
        world.remove::<ActiveShrine>(shrine);
    }
    world.insert_resource(QuestState {
        collected_shards: snapshot.collected_shards,
        shrine_active: snapshot.shrine_active,
    });
    world.insert_resource(ScheduledInput::default());
    world.insert_resource(InputFrame::<Action>::default());
    let text = format!(
        "SHARDS {}/3{}",
        snapshot.collected_shards,
        if snapshot.shrine_active {
            "  SHRINE ACTIVE"
        } else {
            ""
        }
    );
    let labels: Vec<_> = world.iter::<QuestHud>().map(|(e, _)| e).collect();
    for label in labels {
        world
            .get_mut::<UiText>(label)
            .unwrap()
            .text
            .clone_from(&text);
    }
    journal::reset(app.world_mut());
    app.refresh_extracted();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn midquest_restore_rebuilds_shards_and_removes_active_shrine_without_rewinding() {
        let mut app = build_game();
        app.update_schedule(Startup);
        let route = recorded_walk();
        for input in &route.frames()[..2] {
            app.world_mut().insert_resource(input.clone());
            app.advance_fixed(1);
        }
        let saved = export(&app).unwrap();
        for input in &route.frames()[2..] {
            app.world_mut().insert_resource(input.clone());
            app.advance_fixed(1);
        }
        assert!(app.world().iter::<ActiveShrine>().next().is_some());
        let tick = app.world().resource::<FixedTime>().unwrap().tick();
        load(&mut app, saved.clone()).unwrap();
        assert_eq!(export(&app).unwrap(), saved);
        assert!(app.world().iter::<ActiveShrine>().next().is_none());
        assert_eq!(app.world().resource::<FixedTime>().unwrap().tick(), tick);
        for input in &route.frames()[2..] {
            app.world_mut().insert_resource(input.clone());
            app.advance_fixed(1);
        }
        assert_eq!(
            image_checksum(&render_image(app.world()).unwrap()),
            0xf7a298f62ad75c1c
        );
    }
    #[test]
    fn malformed_snapshot_and_target_fail_before_any_gameplay_write() {
        let mut app = build_game();
        app.update_schedule(Startup);
        let before = export(&app).unwrap();
        let mut invalid = before.clone();
        invalid["player"]["x"] = 999.into();
        assert!(load(&mut app, invalid).is_err());
        assert_eq!(export(&app).unwrap(), before);
        let label = app.world().iter::<QuestHud>().next().unwrap().0;
        app.world_mut().remove::<UiText>(label);
        let player = app.world().iter::<Player>().next().unwrap().0;
        let mut changed = before;
        changed["player"]["x"] = 9.into();
        assert!(load(&mut app, changed).is_err());
        assert_eq!(app.world().get::<Position>(player).unwrap().x, 2);
    }
    #[test]
    fn duplicate_spawned_shards_survive_canonical_roundtrip() {
        let mut app = build_game();
        app.update_schedule(Startup);
        spawn_at(
            app.world_mut(),
            Position { x: 3, y: 2 },
            Shard,
            "spawned-shard",
        );
        spawn_at(
            app.world_mut(),
            Position { x: 3, y: 2 },
            Shard,
            "spawned-shard",
        );
        let before = export(&app).unwrap();
        load(&mut app, before.clone()).unwrap();
        assert_eq!(export(&app).unwrap(), before);
    }
}

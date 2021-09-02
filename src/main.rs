use std::collections::VecDeque;

use bevy::prelude::*;
use bevy_ecs_tilemap::prelude::*;

use ferris_lab::spritesheet::{self};
use pathfinding::{directed::astar, num_traits::Zero};

#[derive(Eq, PartialEq, Hash, Clone, Debug)]
struct Ferris {
    pos: UVec2,
    keys: [bool; 3],
}

struct EndPos(UVec2);

#[derive(Default)]
struct ChaseCamera {
    x_moving: bool,
    y_moving: bool,
}

struct ChaseCameraTarget;

fn startup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands
        .spawn_bundle(OrthographicCameraBundle::new_2d())
        .insert(ChaseCamera::default());

    let handle: Handle<LdtkMap> = asset_server.load("labyrinth.ldtk");

    let map_entity = commands.spawn().id();

    commands.entity(map_entity).insert_bundle(LdtkMapBundle {
        ldtk_map: handle,
        map: Map::new(0u16, map_entity),
        transform: Transform::from_xyz(0.0, 0.0, 0.0),
        ..Default::default()
    });
}

fn update_camera(
    mut camera_query: Query<(&mut Transform, &mut ChaseCamera), With<bevy::render::camera::Camera>>,
    target_query: Query<
        &Transform,
        (
            With<ChaseCameraTarget>,
            Without<bevy::render::camera::Camera>,
        ),
    >,
) {
    if let Some(target_transform) = target_query.iter().next() {
        let target_translation = target_transform.translation;
        for (mut camera_transform, mut follow_camera) in camera_query.iter_mut() {
            // TODO: zoom out slightly during movement
            camera_transform.scale = Vec3::new(0.25, 0.25, 1.0);

            let xoffs = target_translation.x - camera_transform.translation.x;
            let yoffs = target_translation.y - camera_transform.translation.y;
            let mut xmove = 0.0;
            let mut ymove = 0.0;
            const CHASE_MAX_DISTANCE: f32 = 64.0;
            // chase with 'hysteresis' per axis:
            // - start chasing target if it is more than CHASE_MAX_DISTANCE from center
            // - if the camera is already chasing keep moving until target is in center
            if xoffs.abs() > CHASE_MAX_DISTANCE || (follow_camera.x_moving && xoffs.abs() >= 1.0) {
                xmove = xoffs.signum();
            }
            if yoffs.abs() > CHASE_MAX_DISTANCE || (follow_camera.y_moving && yoffs.abs() >= 1.0) {
                ymove = yoffs.signum();
            }
            follow_camera.x_moving = xmove.abs() >= 1.0;
            follow_camera.y_moving = ymove.abs() >= 1.0;
            camera_transform.translation.x += xmove;
            camera_transform.translation.y += ymove;
        }
    }
}

fn main() {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    App::new()
        .insert_resource(WindowDescriptor {
            width: 1270.0,
            height: 720.0,
            title: String::from("LDTK Example"),
            ..Default::default()
        })
        .add_plugins(DefaultPlugins)
        .add_plugin(TilemapPlugin)
        .add_plugin(LdtkPlugin)
        .add_startup_system(startup.system())
        .add_system(update_camera.system())
        // .add_system(ferris_lab::camera::movement.system())
        .add_system(ferris_lab::texture::set_texture_filters_to_nearest.system())
        .add_system(init_ferris.system())
        .add_system(move_ferris.system())
        .add_system(process_loaded_tile_maps.system())
        .add_system(character_input.system())
        .add_system(play_solution.system())
        .add_system(show_solution)
        // .add_system(dump_tiles.system())
        .run();
}

fn dump_tiles(tile_query: Query<(&Tile, &UVec2)>) {
    for (tile, pos) in tile_query.iter() {
        println!("{:?} {:?}", tile, pos);
    }
}

fn pos_to_translation(pos: &UVec2) -> Vec3 {
    Vec3::new(
        (pos.x * 16) as f32 + 8.0,
        ((16 - pos.y) * 16) as f32 * -1.0 + 8.0,
        0.0,
    )
}

const START_TILE: u16 = 18;
const END_TILE: u16 = 19;

const LEVEL_ID: u16 = 0;
const LAYER_ID: u16 = 1;

fn init_ferris(
    mut commands: Commands,
    mut query: Query<(Entity, &mut Ferris), Added<Ferris>>,
    tile_query: Query<(&Tile, &UVec2)>,
    asset_server: Res<AssetServer>,
    mut texture_atlases: ResMut<Assets<TextureAtlas>>,
    mut map_query: MapQuery,
) {
    for (entity, mut ferris) in query.iter_mut() {
        let mut start_pos = Default::default();
        let mut end_pos = Default::default();

        for (tile, pos) in tile_query.iter() {
            match tile.texture_index {
                START_TILE => start_pos = *pos,
                END_TILE => end_pos = *pos,
                _ => (),
            }
        }

        info!("ferris added {:?} at {:?}", entity, start_pos);

        let desc: Handle<spritesheet::Spritesheet> = asset_server.load("ferris2.0.json");
        let texture_handle = asset_server.load("ferris2.0.png");
        let texture_atlas = TextureAtlas::from_grid(texture_handle, Vec2::new(16.0, 16.0), 10, 1);
        let texture_atlas_handle = texture_atlases.add(texture_atlas);

        let mut timer = Timer::from_seconds(0.2, true);
        // timer.pause();

        commands
            .entity(entity)
            .insert_bundle(SpriteSheetBundle {
                texture_atlas: texture_atlas_handle,
                transform: Transform {
                    scale: Vec3::splat(1.0),
                    ..Default::default()
                },
                ..Default::default()
            })
            .insert(desc)
            //            .insert(solution)
            .insert(EndPos(end_pos))
            .insert(timer);
        ferris.pos = start_pos;
        // commands.entity(entity).insert_bundle
    }
}

fn solve(
    map_query: &mut MapQuery,
    start_state: Ferris,
    end_pos: &UVec2,
    query: &Query<(&Tile, &UVec2)>,
) -> VecDeque<Ferris> {
    let successors = |state: &Ferris| {
        let neigbors = map_query.get_tile_neighbors(state.pos, LEVEL_ID, LAYER_ID);

        let mut successors = Vec::new();
        for (pos, tile) in neigbors.iter().take(4) {
            let mut new_state = state.clone();
            new_state.pos = pos.as_u32();

            if let Some(tile) = tile {
                if let Ok((tile, _)) = query.get(*tile) {
                    if tile.texture_index == END_TILE
                        || ((2..=4).contains(&tile.texture_index)
                            && new_state.keys[(tile.texture_index - 2) as usize])
                    {
                        successors.push((new_state, 1));
                    } else if (5..=7).contains(&tile.texture_index) {
                        new_state.keys[(tile.texture_index - 5) as usize] = true;
                        successors.push((new_state, 1));
                    }
                }
            } else {
                successors.push((new_state, 1));
            }
        }
        successors
    };
    let heuristic = |state: &Ferris| {
        let d = end_pos.as_i32() - state.pos.as_i32();
        d.x.abs() + d.y.abs()
    };
    let success = |state: &Ferris| state.pos == *end_pos;
    let res = astar::astar(&start_state, successors, heuristic, success);

    if let Some(res) = res {
        // info!("len: {}", res.1);
        // for state in res.0.iter() {
        //     info!("{:?}", state);
        // }
        res.0.iter().cloned().collect()
    } else {
        error!("no path found");
        VecDeque::new()
    }
}

fn show_solution(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    solution_query: Query<&VecDeque<Ferris>, Added<VecDeque<Ferris>>>,
) {
    for solution in solution_query.iter() {
        let texture_handle = asset_server.load("bread_crumb.png");

        for state in solution.iter() {
            info!("{:?}", state);
            commands.spawn_bundle(SpriteBundle {
                material: materials.add(texture_handle.clone().into()),
                transform: Transform::from_translation(pos_to_translation(&state.pos)),
                ..Default::default()
            });
        }
    }
}

fn is_walkable_tile(texture_index: u16) -> bool {
    let res = (5..=7).contains(&texture_index)
        || texture_index == START_TILE
        || texture_index == END_TILE;
    info!("{} {}", texture_index, res);
    res
}

fn character_input(
    mut commands: Commands,
    keyboard_input: Res<Input<KeyCode>>,
    mut query: Query<(Entity, &mut Ferris, &mut Timer, &EndPos)>,
    tile_query: Query<(&Tile, &UVec2)>,
    mut map_query: MapQuery,
) {
    for (ferris_entity, mut ferris, mut timer, end_pos) in query.iter_mut() {
        let mut new_x = ferris.pos.x as i32;
        let mut new_y = ferris.pos.y as i32;
        for key_code in keyboard_input.get_just_pressed() {
            match key_code {
                KeyCode::Up => new_y += 1,
                KeyCode::Down => new_y -= 1,
                KeyCode::Left => new_x -= 1,
                KeyCode::Right => new_x += 1,
                KeyCode::R => {
                    let solution = solve(&mut map_query, ferris.clone(), &end_pos.0, &tile_query);
                    commands.entity(ferris_entity).insert(solution);
                }
                _ => (),
            }
        }

        new_x = new_x.clamp(0, 15);
        new_y = new_y.clamp(0, 15);
        let mut can_move = true;
        let mut despawn = false;
        let new_pos = UVec2::new(new_x as u32, new_y as u32);
        if let Ok(tile_ent) = map_query.get_tile_entity(new_pos, LEVEL_ID, LAYER_ID) {
            if let Ok((tile, _)) = tile_query.get(tile_ent) {
                if (5..=7).contains(&tile.texture_index) {
                    ferris.keys[(tile.texture_index - 5) as usize] = true;
                    despawn = true;
                }
                can_move = is_walkable_tile(tile.texture_index);
                if (2..=4).contains(&tile.texture_index) {
                    can_move = ferris.keys[(tile.texture_index - 2) as usize];
                    despawn = can_move;
                }
            }
        }

        if despawn {
            map_query.despawn_tile(&mut commands, new_pos, LEVEL_ID, LAYER_ID);
            map_query.notify_chunk_for_tile(new_pos, LEVEL_ID, LAYER_ID);
        }
        if can_move {
            ferris.pos = new_pos;
        }
    }
}

fn play_solution(
    mut query: Query<(&mut Ferris, &mut VecDeque<Ferris>, &mut Timer)>,
    time: Res<Time>,
) {
    for (mut ferris, mut solution, mut timer) in query.iter_mut() {
        timer.tick(time.delta());
        if !solution.is_empty() && timer.just_finished() {
            *ferris = solution.pop_front().unwrap();
        }
    }
}

fn move_ferris(mut query: Query<(&Ferris, &mut Transform)>) {
    for (ferris, mut transform) in query.iter_mut() {
        transform.translation = pos_to_translation(&ferris.pos);
    }
}

// fn solve(mut map_query: MapQuery, query: Query<(&Ferris)>) {}

fn process_loaded_tile_maps(
    mut commands: Commands,
    mut map_events: EventReader<AssetEvent<LdtkMap>>,
    maps: Res<Assets<LdtkMap>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut query: Query<(Entity, &Handle<LdtkMap>, &mut Map)>,
    new_maps: Query<&Handle<LdtkMap>, Added<Handle<LdtkMap>>>,
    layer_query: Query<&Layer>,
    chunk_query: Query<&Chunk>,
    ferris_query: Query<(Entity, &Ferris)>,
) {
    let mut changed_maps = Vec::<Handle<LdtkMap>>::default();
    for event in map_events.iter() {
        match event {
            AssetEvent::Created { handle } => {
                log::info!("Map added!");
                changed_maps.push(handle.clone());
            }
            AssetEvent::Modified { handle } => {
                log::info!("Map changed!");
                changed_maps.push(handle.clone());
            }
            AssetEvent::Removed { handle } => {
                log::info!("Map removed!");
                // if mesh was modified and removed in the same update, ignore the modification
                // events are ordered so future modification events are ok
                changed_maps = changed_maps
                    .into_iter()
                    .filter(|changed_handle| changed_handle == handle)
                    .collect();
            }
        }
    }

    // If we have new map entities add them to the changed_maps list.
    for new_map_handle in new_maps.iter() {
        changed_maps.push(new_map_handle.clone());
    }

    for changed_map in changed_maps.iter() {
        for (_, map_handle, mut map) in query.iter_mut() {
            // only deal with currently changed map
            if map_handle != changed_map {
                continue;
            }
        }

        info!("changed map: {:?}", changed_map);
        if let Some(ldtk_map) = maps.get(changed_map) {
            let layers = ldtk_map
                .project
                .get_level(0)
                .unwrap()
                .layer_instances
                .as_ref()
                .unwrap();

            for layer in layers {
                info!("layer: {} {}", layer.identifier, layer.layer_def_uid);
            }
        }

        for (entity, _) in ferris_query.iter() {
            commands.entity(entity).despawn();
        }

        commands
            .spawn()
            .insert(Ferris {
                pos: UVec2::splat(0),
                keys: [false; 3],
            })
            .insert(ChaseCameraTarget);
    }
}

//! Headless 3D authoring/extraction fixture, also executed as actual WASM in CI.
use titan::render::three_d::*;
use titan::{App, Component, World};

#[derive(Component)]
struct Object(Draw3d);

fn extract(world: &World) -> Result<RenderFrame3d, Frame3dError> {
    RenderFrame3d::new(
        *world.resource::<PerspectiveCamera>().unwrap(),
        *world.resource::<Lighting3d>().unwrap(),
        world.resource::<MeshAssets>().unwrap(),
        world.iter::<Object>().map(|(_, object)| object.0),
        Frame3dLimits::default(),
    )
}

fn verify() {
    let mut app = App::new();
    let mut assets = MeshAssets::new();
    let cube = assets.insert(Mesh::cube(1.0).unwrap()).unwrap();
    let floor = assets.insert(Mesh::floor(8.0).unwrap()).unwrap();
    let camera = PerspectiveCamera::new(
        Vec3::new(0.0, 2.0, 5.0),
        Quaternion::IDENTITY,
        std::f32::consts::FRAC_PI_3,
        16.0 / 9.0,
        0.1,
        100.0,
    )
    .unwrap();
    // Near and far planes map to the agreed depth interval in actual WASM too.
    for (distance, depth) in [(0.1, 0.0), (100.0, 1.0)] {
        let clip = camera
            .projection_matrix()
            .transform([0.0, 0.0, -distance, 1.0])
            .unwrap();
        assert!((clip[2] / clip[3] - depth).abs() < 1e-5);
    }
    let world = app.world_mut();
    world.insert_resource(assets);
    world.insert_resource(camera);
    world.insert_resource(Lighting3d::new(Vec3::new(1.0, 2.0, 3.0), 0.2, 0.8).unwrap());
    world.spawn_with((Object(Draw3d {
        mesh: cube,
        transform: Transform3d::new(Vec3::new(0.0, 0.5, 0.0), Quaternion::IDENTITY, Vec3::ONE)
            .unwrap(),
        color: BaseColor::rgb(220, 80, 40),
        order: 20,
    }),));
    world.spawn_with((Object(Draw3d {
        mesh: floor,
        transform: Transform3d::identity(),
        color: BaseColor::rgb(140, 160, 180),
        order: 10,
    }),));
    app.add_extractor(extract);
    app.update();
    let before = app
        .extracted::<Result<RenderFrame3d, Frame3dError>>()
        .unwrap()
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(
        before
            .draws()
            .iter()
            .map(|d| d.draw().order)
            .collect::<Vec<_>>(),
        [10, 20]
    );
    assert_eq!(before.draws()[1].mesh().positions().len(), 24);
    app.world_mut()
        .resource_mut::<MeshAssets>()
        .unwrap()
        .remove(cube)
        .unwrap();
    app.refresh_extracted();
    assert!(matches!(
        app.extracted::<Result<RenderFrame3d, Frame3dError>>()
            .unwrap(),
        Err(Frame3dError::Asset(_))
    ));
    assert_eq!(before.draws()[1].mesh().positions().len(), 24);
    // Collection replacement cannot resolve the old numeric handle.
    let mut replacement = MeshAssets::new();
    replacement.insert(Mesh::cube(2.0).unwrap()).unwrap();
    assert!(replacement.get(cube).is_err());
    assert!(Mesh::new(vec![Vec3::ZERO; 3], vec![Vec3::ONE; 3], vec![0, 1, 9]).is_err());
    assert!(PerspectiveCamera::new(Vec3::ZERO, Quaternion::IDENTITY, 0.0, 1.0, 0.1, 1.0).is_err());
}

// Export only on WASM; no browser adapter, imported host functions, or GPU needed.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn verify_render_3d() -> u32 {
    verify();
    43
}

fn main() {
    verify();
}

#[test]
fn headless_3d_public_boundary() {
    verify();
}

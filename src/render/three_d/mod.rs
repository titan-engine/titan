//! Validated, CPU-resident 3D data. No GPU, window, or software rasterizer.
//!
//! Right-handed coordinates, +Y up, camera forward -Z, column vectors,
//! counterclockwise front faces and perspective depth in 0..=1.

mod math;
mod mesh;

pub use math::{Mat4, MathError, PerspectiveCamera, Quaternion, Transform3d, Vec3};
pub use mesh::{
    MAX_MESH_ASSETS, MAX_MESH_BYTES, MAX_MESH_INDICES, MAX_MESH_VERTICES, Mesh, MeshAssetError,
    MeshAssets, MeshError, MeshHandle,
};

use std::{collections::BTreeSet, error::Error, fmt, sync::Arc};

/// Opaque sRGB authoring color. Decode to linear before 3D lighting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaseColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl BaseColor {
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    pub fn linear(self) -> [f32; 3] {
        [self.red, self.green, self.blue].map(|byte| {
            let c = f32::from(byte) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        })
    }
}

/// One white directional light, with bounded ambient and diffuse intensities.
/// Lighting is `base_linear * clamp(ambient + diffuse * max(N·L, 0), 0, 1)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting3d {
    to_light: Vec3,
    ambient: f32,
    diffuse: f32,
}

impl Lighting3d {
    pub fn new(to_light: Vec3, ambient: f32, diffuse: f32) -> Result<Self, Frame3dError> {
        let to_light = to_light.normalized().map_err(Frame3dError::Math)?;
        if ![ambient, diffuse]
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        {
            return Err(Frame3dError::InvalidLighting);
        }
        Ok(Self {
            to_light,
            ambient,
            diffuse,
        })
    }

    pub const fn to_light(self) -> Vec3 {
        self.to_light
    }
    pub const fn ambient(self) -> f32 {
        self.ambient
    }
    pub const fn diffuse(self) -> f32 {
        self.diffuse
    }
}

/// Game-authored draw. `order` is a unique stable key within one frame.
/// For entities, `(u64::from(entity.index()) << 32) | u64::from(entity.generation())`
/// provides a deterministic tie break even for surfaces at equal depth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Draw3d {
    pub mesh: MeshHandle,
    pub transform: Transform3d,
    pub color: BaseColor,
    pub order: u64,
}

/// A resolved draw retains its exact immutable mesh version.
#[derive(Clone, Debug)]
pub struct FrameDraw3d {
    draw: Draw3d,
    mesh: Arc<Mesh>,
}

impl FrameDraw3d {
    pub const fn draw(&self) -> &Draw3d {
        &self.draw
    }
    pub fn mesh(&self) -> &Mesh {
        &self.mesh
    }
}

pub const MAX_FRAME_DRAWS: usize = 65_536;
pub const MAX_FRAME_GEOMETRY_BYTES: usize = 256 * 1024 * 1024;

/// Per-frame budgets, optionally lower than the hard caps. Geometry is charged
/// once per draw (even when shared), conservatively bounding retained/upload data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame3dLimits {
    pub max_draws: usize,
    pub max_geometry_bytes: usize,
}

impl Default for Frame3dLimits {
    fn default() -> Self {
        Self {
            max_draws: MAX_FRAME_DRAWS,
            max_geometry_bytes: MAX_FRAME_GEOMETRY_BYTES,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Frame3dError {
    Math(MathError),
    Asset(MeshAssetError),
    InvalidLighting,
    InvalidLimits,
    TooManyDraws,
    TooMuchGeometry,
    DuplicateOrder(u64),
}

impl fmt::Display for Frame3dError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Math(e) => write!(f, "invalid 3D math: {e}"),
            Self::Asset(e) => write!(f, "invalid 3D asset: {e}"),
            Self::InvalidLighting => write!(f, "light intensities must be finite and in 0..=1"),
            Self::InvalidLimits => write!(f, "frame limits exceed hard caps"),
            Self::TooManyDraws => write!(f, "frame draw count exceeds budget"),
            Self::TooMuchGeometry => write!(f, "frame geometry bytes exceed budget"),
            Self::DuplicateOrder(key) => write!(f, "duplicate frame draw order {key}"),
        }
    }
}

impl Error for Frame3dError {}

/// Owned snapshot built from immutable game state using `App::add_extractor`.
/// Construction resolves handles, checks budgets, and sorts by unique draw key.
/// Later asset/world changes cannot alter this frame. Errors return no partial frame.
#[derive(Clone, Debug)]
pub struct RenderFrame3d {
    camera: PerspectiveCamera,
    lighting: Lighting3d,
    draws: Vec<FrameDraw3d>,
    geometry_bytes: usize,
}

impl RenderFrame3d {
    pub fn new(
        camera: PerspectiveCamera,
        lighting: Lighting3d,
        assets: &MeshAssets,
        draws: impl IntoIterator<Item = Draw3d>,
        limits: Frame3dLimits,
    ) -> Result<Self, Frame3dError> {
        if limits.max_draws > MAX_FRAME_DRAWS
            || limits.max_geometry_bytes > MAX_FRAME_GEOMETRY_BYTES
        {
            return Err(Frame3dError::InvalidLimits);
        }
        let mut resolved = Vec::new();
        let mut orders = BTreeSet::new();
        let mut geometry_bytes = 0usize;
        for draw in draws {
            if resolved.len() == limits.max_draws {
                return Err(Frame3dError::TooManyDraws);
            }
            if !orders.insert(draw.order) {
                return Err(Frame3dError::DuplicateOrder(draw.order));
            }
            let mesh = assets.get(draw.mesh).map_err(Frame3dError::Asset)?;
            geometry_bytes = geometry_bytes
                .checked_add(mesh.geometry_bytes())
                .filter(|bytes| *bytes <= limits.max_geometry_bytes)
                .ok_or(Frame3dError::TooMuchGeometry)?;
            // Validate the composed matrix too: finite inputs can still overflow.
            camera
                .projection_matrix()
                .checked_mul(camera.view_matrix())
                .and_then(|vp| vp.checked_mul(draw.transform.matrix()))
                .map_err(Frame3dError::Math)?;
            resolved.push(FrameDraw3d { draw, mesh });
        }
        resolved.sort_unstable_by_key(|resolved| resolved.draw.order);
        Ok(Self {
            camera,
            lighting,
            draws: resolved,
            geometry_bytes,
        })
    }

    pub const fn camera(&self) -> &PerspectiveCamera {
        &self.camera
    }
    pub const fn lighting(&self) -> &Lighting3d {
        &self.lighting
    }
    pub fn draws(&self) -> &[FrameDraw3d] {
        &self.draws
    }
    pub const fn geometry_bytes(&self) -> usize {
        self.geometry_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> PerspectiveCamera {
        PerspectiveCamera::new(Vec3::ZERO, Quaternion::IDENTITY, 1.0, 1.0, 0.1, 100.0).unwrap()
    }
    fn light() -> Lighting3d {
        Lighting3d::new(Vec3::ONE, 0.2, 0.8).unwrap()
    }
    fn draw(mesh: MeshHandle, order: u64) -> Draw3d {
        Draw3d {
            mesh,
            transform: Transform3d::identity(),
            color: BaseColor::WHITE,
            order,
        }
    }

    #[test]
    fn sorted_snapshot_retains_assets_after_replacement_and_collection_drop() {
        let mut assets = MeshAssets::new();
        let mesh = assets.insert(Mesh::cube(2.0).unwrap()).unwrap();
        let frame = RenderFrame3d::new(
            camera(),
            light(),
            &assets,
            [draw(mesh, 9), draw(mesh, 1), draw(mesh, 5)],
            Frame3dLimits::default(),
        )
        .unwrap();
        let reverse = RenderFrame3d::new(
            camera(),
            light(),
            &assets,
            [draw(mesh, 5), draw(mesh, 1), draw(mesh, 9)],
            Frame3dLimits::default(),
        )
        .unwrap();
        assert_eq!(
            frame.draws().iter().map(|d| *d.draw()).collect::<Vec<_>>(),
            reverse
                .draws()
                .iter()
                .map(|d| *d.draw())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            frame
                .draws()
                .iter()
                .map(|d| d.draw().order)
                .collect::<Vec<_>>(),
            [1, 5, 9]
        );
        assets.replace(mesh, Mesh::floor(4.0).unwrap()).unwrap();
        assert!(matches!(
            RenderFrame3d::new(
                camera(),
                light(),
                &assets,
                [draw(mesh, 0)],
                Frame3dLimits::default()
            ),
            Err(Frame3dError::Asset(_))
        ));
        drop(assets);
        assert_eq!(frame.draws()[0].mesh().positions().len(), 24);
        assert_eq!(frame.draws()[0].mesh().positions()[0].z, 1.0);
    }

    #[test]
    fn budgets_and_duplicate_orders_reject_without_partial_frames() {
        let mut assets = MeshAssets::new();
        let mesh = assets.insert(Mesh::floor(1.0).unwrap()).unwrap();
        let bytes = assets.get(mesh).unwrap().geometry_bytes();
        let build = |draws: Vec<Draw3d>, limits| {
            RenderFrame3d::new(camera(), light(), &assets, draws, limits)
        };
        let limits = Frame3dLimits {
            max_draws: 2,
            max_geometry_bytes: bytes * 2,
        };
        assert_eq!(
            build(vec![draw(mesh, 0), draw(mesh, 1)], limits)
                .unwrap()
                .geometry_bytes(),
            bytes * 2
        );
        assert_eq!(
            build(vec![draw(mesh, 0), draw(mesh, 1), draw(mesh, 2)], limits).unwrap_err(),
            Frame3dError::TooManyDraws
        );
        assert_eq!(
            build(
                vec![draw(mesh, 0), draw(mesh, 1)],
                Frame3dLimits {
                    max_geometry_bytes: bytes * 2 - 1,
                    ..limits
                }
            )
            .unwrap_err(),
            Frame3dError::TooMuchGeometry
        );
        assert_eq!(
            build(vec![draw(mesh, 1), draw(mesh, 1)], limits).unwrap_err(),
            Frame3dError::DuplicateOrder(1)
        );
        assert_eq!(
            build(
                vec![],
                Frame3dLimits {
                    max_draws: usize::MAX,
                    ..limits
                }
            )
            .unwrap_err(),
            Frame3dError::InvalidLimits
        );
        assert_eq!(
            build(
                vec![],
                Frame3dLimits {
                    max_geometry_bytes: usize::MAX,
                    ..limits
                }
            )
            .unwrap_err(),
            Frame3dError::InvalidLimits
        );
        assert!(
            build(
                vec![],
                Frame3dLimits {
                    max_draws: 0,
                    max_geometry_bytes: 0
                }
            )
            .unwrap()
            .draws()
            .is_empty()
        );
    }

    #[test]
    fn lighting_and_srgb_are_validated() {
        for bad in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            assert!(Lighting3d::new(Vec3::ONE, bad, 1.0).is_err());
            assert!(Lighting3d::new(Vec3::ONE, 1.0, bad).is_err());
        }
        assert!(Lighting3d::new(Vec3::ZERO, 0.0, 1.0).is_err());
        assert!(Lighting3d::new(Vec3::new(f32::NAN, 1.0, 1.0), 0.0, 1.0).is_err());
        let direction = light().to_light();
        assert!(
            (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z
                - 1.0)
                .abs()
                < 1e-6
        );
        assert_eq!(BaseColor::WHITE.linear(), [1.0; 3]);
        assert_eq!(BaseColor::rgb(0, 0, 0).linear(), [0.0; 3]);
        assert!((BaseColor::rgb(128, 10, 0).linear()[0] - 0.2158605).abs() < 1e-6);
        assert!((BaseColor::rgb(128, 10, 0).linear()[1] - 0.00303527).abs() < 1e-6);
    }
}

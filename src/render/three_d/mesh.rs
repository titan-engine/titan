//! Validated CPU geometry and collection-scoped, versioned mesh assets.

use super::math::Vec3;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

pub const MAX_MESH_VERTICES: usize = 1_000_000;
pub const MAX_MESH_INDICES: usize = 3_000_000;
pub const MAX_MESH_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_MESH_ASSETS: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshError {
    Empty,
    AttributeLengths,
    IncompleteTriangle,
    LimitsExceeded,
    NonFinitePosition,
    InvalidNormal,
    IndexOutOfRange,
    DegenerateTriangle,
    InvalidSize,
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid mesh: {self:?}")
    }
}
impl std::error::Error for MeshError {}

/// Immutable indexed triangles. Normals need not be unit length but must be
/// finite and nonzero; renderers normalize after transforming them.
#[derive(Debug)]
pub struct Mesh {
    positions: Box<[Vec3]>,
    normals: Box<[Vec3]>,
    indices: Box<[u32]>,
    geometry_bytes: usize,
}

impl Mesh {
    pub fn new(
        positions: Vec<Vec3>,
        normals: Vec<Vec3>,
        indices: Vec<u32>,
    ) -> Result<Self, MeshError> {
        if positions.is_empty() || indices.is_empty() {
            return Err(MeshError::Empty);
        }
        if positions.len() != normals.len() {
            return Err(MeshError::AttributeLengths);
        }
        if !indices.len().is_multiple_of(3) {
            return Err(MeshError::IncompleteTriangle);
        }
        let geometry_bytes = geometry_size(positions.len(), indices.len())?;
        if positions.iter().any(|p| !finite(*p)) {
            return Err(MeshError::NonFinitePosition);
        }
        if normals
            .iter()
            .any(|n| !finite(*n) || (n.x == 0.0 && n.y == 0.0 && n.z == 0.0))
        {
            return Err(MeshError::InvalidNormal);
        }
        if indices.iter().any(|&i| i as usize >= positions.len()) {
            return Err(MeshError::IndexOutOfRange);
        }
        for triangle in indices.as_chunks::<3>().0 {
            let a = positions[triangle[0] as usize];
            let b = positions[triangle[1] as usize];
            let c = positions[triangle[2] as usize];
            // f64 differences/products prevent overflow or underflow across the
            // entire finite f32 input range, including subnormal coordinates.
            let ab = [
                b.x as f64 - a.x as f64,
                b.y as f64 - a.y as f64,
                b.z as f64 - a.z as f64,
            ];
            let ac = [
                c.x as f64 - a.x as f64,
                c.y as f64 - a.y as f64,
                c.z as f64 - a.z as f64,
            ];
            let cross = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            if cross == [0.0; 3] {
                return Err(MeshError::DegenerateTriangle);
            }
        }
        Ok(Self {
            positions: positions.into_boxed_slice(),
            normals: normals.into_boxed_slice(),
            indices: indices.into_boxed_slice(),
            geometry_bytes,
        })
    }

    pub fn positions(&self) -> &[Vec3] {
        &self.positions
    }
    pub fn normals(&self) -> &[Vec3] {
        &self.normals
    }
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }
    /// Bytes of tightly packed f32 positions/normals and u32 indices. This is
    /// the geometry upload budget, excluding allocator/Arc bookkeeping.
    pub fn geometry_bytes(&self) -> usize {
        self.geometry_bytes
    }

    /// A cube centered at the origin, with outward flat normals and CCW faces.
    pub fn cube(size: f32) -> Result<Self, MeshError> {
        let h = half_size(size)?;
        let mut positions = Vec::with_capacity(24);
        let mut normals = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);
        let faces = [
            (
                [[-h, -h, h], [h, -h, h], [h, h, h], [-h, h, h]],
                Vec3::new(0.0, 0.0, 1.0),
            ),
            (
                [[h, -h, -h], [-h, -h, -h], [-h, h, -h], [h, h, -h]],
                Vec3::new(0.0, 0.0, -1.0),
            ),
            (
                [[h, -h, h], [h, -h, -h], [h, h, -h], [h, h, h]],
                Vec3::new(1.0, 0.0, 0.0),
            ),
            (
                [[-h, -h, -h], [-h, -h, h], [-h, h, h], [-h, h, -h]],
                Vec3::new(-1.0, 0.0, 0.0),
            ),
            (
                [[-h, h, h], [h, h, h], [h, h, -h], [-h, h, -h]],
                Vec3::new(0.0, 1.0, 0.0),
            ),
            (
                [[-h, -h, -h], [h, -h, -h], [h, -h, h], [-h, -h, h]],
                Vec3::new(0.0, -1.0, 0.0),
            ),
        ];
        for (vertices, normal) in faces {
            let base = positions.len() as u32;
            positions.extend(vertices.map(|p| Vec3::new(p[0], p[1], p[2])));
            normals.extend([normal; 4]);
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        Self::new(positions, normals, indices)
    }

    /// A square in the XZ plane centered at the origin, facing +Y.
    pub fn floor(size: f32) -> Result<Self, MeshError> {
        let h = half_size(size)?;
        Self::new(
            vec![
                Vec3::new(-h, 0.0, h),
                Vec3::new(h, 0.0, h),
                Vec3::new(h, 0.0, -h),
                Vec3::new(-h, 0.0, -h),
            ],
            vec![Vec3::new(0.0, 1.0, 0.0); 4],
            vec![0, 1, 2, 0, 2, 3],
        )
    }
}

fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}
fn half_size(size: f32) -> Result<f32, MeshError> {
    let half = size * 0.5;
    if !size.is_finite() || half <= 0.0 {
        Err(MeshError::InvalidSize)
    } else {
        Ok(half)
    }
}
fn geometry_size(vertices: usize, indices: usize) -> Result<usize, MeshError> {
    let bytes = vertices
        .checked_mul(24)
        .and_then(|v| indices.checked_mul(4).and_then(|i| v.checked_add(i)))
        .ok_or(MeshError::LimitsExceeded)?;
    if vertices > MAX_MESH_VERTICES || indices > MAX_MESH_INDICES || bytes > MAX_MESH_BYTES {
        Err(MeshError::LimitsExceeded)
    } else {
        Ok(bytes)
    }
}

/// Process-local identity; not a serialized asset identifier. Handles from
/// another collection or an earlier mesh version never resolve here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshHandle {
    collection: u32,
    slot: u32,
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshAssetError {
    MissingOrStale,
    LimitsExceeded,
    GenerationExhausted,
}
impl fmt::Display for MeshAssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mesh asset error: {self:?}")
    }
}
impl std::error::Error for MeshAssetError {}

struct Slot {
    generation: u64,
    mesh: Option<Arc<Mesh>>,
}

/// Owns immutable versions. Replacement returns a new handle and invalidates
/// the old handle; any previously returned Arc continues to own its version.
pub struct MeshAssets {
    collection: u32,
    slots: Vec<Slot>,
}
static NEXT_COLLECTION: AtomicU32 = AtomicU32::new(1);
impl Default for MeshAssets {
    fn default() -> Self {
        Self::new()
    }
}
impl MeshAssets {
    /// Panics if all process-local collection identities have been consumed.
    /// The counter never wraps or reuses an identity, including on wasm32.
    pub fn new() -> Self {
        let collection = NEXT_COLLECTION
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("mesh collection identity exhausted");
        Self {
            collection,
            slots: Vec::new(),
        }
    }
    pub fn insert(&mut self, mesh: Mesh) -> Result<MeshHandle, MeshAssetError> {
        let reusable = self
            .slots
            .iter()
            .position(|s| s.mesh.is_none() && s.generation < u64::MAX);
        let slot = if let Some(index) = reusable {
            self.slots[index].generation += 1;
            self.slots[index].mesh = Some(Arc::new(mesh));
            index
        } else {
            if self.slots.len() >= MAX_MESH_ASSETS {
                return Err(MeshAssetError::LimitsExceeded);
            }
            self.slots.push(Slot {
                generation: 1,
                mesh: Some(Arc::new(mesh)),
            });
            self.slots.len() - 1
        };
        Ok(MeshHandle {
            collection: self.collection,
            slot: slot as u32,
            generation: self.slots[slot].generation,
        })
    }
    pub fn get(&self, handle: MeshHandle) -> Result<Arc<Mesh>, MeshAssetError> {
        Ok(Arc::clone(
            self.slot(handle)?
                .mesh
                .as_ref()
                .expect("validated occupied slot"),
        ))
    }
    pub fn remove(&mut self, handle: MeshHandle) -> Result<Arc<Mesh>, MeshAssetError> {
        self.slot(handle)?;
        Ok(self.slots[handle.slot as usize]
            .mesh
            .take()
            .expect("validated occupied slot"))
    }
    pub fn replace(
        &mut self,
        handle: MeshHandle,
        mesh: Mesh,
    ) -> Result<MeshHandle, MeshAssetError> {
        let generation = self
            .slot(handle)?
            .generation
            .checked_add(1)
            .ok_or(MeshAssetError::GenerationExhausted)?;
        let slot = &mut self.slots[handle.slot as usize];
        slot.mesh = Some(Arc::new(mesh));
        slot.generation = generation;
        Ok(MeshHandle {
            generation,
            ..handle
        })
    }
    fn slot(&self, handle: MeshHandle) -> Result<&Slot, MeshAssetError> {
        if handle.collection != self.collection {
            return Err(MeshAssetError::MissingOrStale);
        }
        self.slots
            .get(handle.slot as usize)
            .filter(|s| s.generation == handle.generation && s.mesh.is_some())
            .ok_or(MeshAssetError::MissingOrStale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle() -> (Vec<Vec3>, Vec<Vec3>, Vec<u32>) {
        (
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            vec![Vec3::new(0.0, 0.0, 1.0); 3],
            vec![0, 1, 2],
        )
    }
    #[test]
    fn rejects_malformed_geometry() {
        let (p, n, i) = triangle();
        assert_eq!(
            Mesh::new(vec![], vec![], vec![]).unwrap_err(),
            MeshError::Empty
        );
        assert_eq!(
            Mesh::new(p.clone(), vec![], i.clone()).unwrap_err(),
            MeshError::AttributeLengths
        );
        assert_eq!(
            Mesh::new(p.clone(), n.clone(), vec![0, 1]).unwrap_err(),
            MeshError::IncompleteTriangle
        );
        assert_eq!(
            Mesh::new(p.clone(), n.clone(), vec![0, 1, 3]).unwrap_err(),
            MeshError::IndexOutOfRange
        );
        assert_eq!(
            Mesh::new(p.clone(), n.clone(), vec![0, 1, 1]).unwrap_err(),
            MeshError::DegenerateTriangle
        );
        let mut bad = p.clone();
        bad[0].x = f32::NAN;
        assert_eq!(
            Mesh::new(bad, n.clone(), i.clone()).unwrap_err(),
            MeshError::NonFinitePosition
        );
        for normal in [Vec3::new(0.0, 0.0, 0.0), Vec3::new(f32::INFINITY, 0.0, 1.0)] {
            assert_eq!(
                Mesh::new(p.clone(), vec![normal; 3], i.clone()).unwrap_err(),
                MeshError::InvalidNormal
            );
        }
        assert_eq!(geometry_size(usize::MAX, 3), Err(MeshError::LimitsExceeded));
        assert_eq!(geometry_size(3, usize::MAX), Err(MeshError::LimitsExceeded));
        assert_eq!(
            geometry_size(MAX_MESH_VERTICES + 1, 3),
            Err(MeshError::LimitsExceeded)
        );
        assert_eq!(
            geometry_size(3, MAX_MESH_INDICES + 1),
            Err(MeshError::LimitsExceeded)
        );
    }
    #[test]
    fn extreme_finite_geometry_does_not_overflow_or_underflow() {
        for scale in [f32::MAX, f32::from_bits(1)] {
            let (_, n, i) = triangle();
            Mesh::new(
                vec![
                    Vec3::new(-scale, 0.0, 0.0),
                    Vec3::new(scale, 0.0, 0.0),
                    Vec3::new(0.0, scale, 0.0),
                ],
                n,
                i,
            )
            .unwrap();
        }
        let (_, n, i) = triangle();
        assert_eq!(
            Mesh::new(
                vec![
                    Vec3::new(-f32::MAX, 0.0, 0.0),
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(f32::MAX, 0.0, 0.0)
                ],
                n,
                i
            )
            .unwrap_err(),
            MeshError::DegenerateTriangle
        );
    }
    #[test]
    fn generated_geometry_winds_outward() {
        for mesh in [Mesh::cube(2.0).unwrap(), Mesh::floor(2.0).unwrap()] {
            for tri in mesh.indices().as_chunks::<3>().0 {
                let a = mesh.positions()[tri[0] as usize];
                let b = mesh.positions()[tri[1] as usize];
                let c = mesh.positions()[tri[2] as usize];
                let n = mesh.normals()[tri[0] as usize];
                let ab = [b.x - a.x, b.y - a.y, b.z - a.z];
                let ac = [c.x - a.x, c.y - a.y, c.z - a.z];
                let dot = (ab[1] * ac[2] - ab[2] * ac[1]) * n.x
                    + (ab[2] * ac[0] - ab[0] * ac[2]) * n.y
                    + (ab[0] * ac[1] - ab[1] * ac[0]) * n.z;
                assert!(dot > 0.0);
            }
            assert_eq!(
                mesh.geometry_bytes(),
                mesh.positions().len() * 24 + mesh.indices().len() * 4
            );
        }
        let cube = Mesh::cube(2.0).unwrap();
        assert_eq!((cube.positions().len(), cube.indices().len()), (24, 36));
        for (p, n) in cube.positions().iter().zip(cube.normals()) {
            assert!(p.x * n.x + p.y * n.y + p.z * n.z > 0.0);
        }
        for size in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::from_bits(1)] {
            assert_eq!(Mesh::cube(size).unwrap_err(), MeshError::InvalidSize);
            assert_eq!(Mesh::floor(size).unwrap_err(), MeshError::InvalidSize);
        }
    }
    #[test]
    fn handles_are_collection_scoped_and_snapshots_keep_versions() {
        let mut assets = MeshAssets::new();
        let first = assets.insert(Mesh::cube(2.0).unwrap()).unwrap();
        let retained = assets.get(first).unwrap();
        let second = assets.replace(first, Mesh::floor(4.0).unwrap()).unwrap();
        assert_ne!(first, second);
        assert!(matches!(
            assets.get(first),
            Err(MeshAssetError::MissingOrStale)
        ));
        assert_eq!(retained.positions().len(), 24);
        let removed = assets.remove(second).unwrap();
        assert_eq!(removed.positions().len(), 4);
        assert!(assets.remove(second).is_err());
        let third = assets.insert(Mesh::cube(8.0).unwrap()).unwrap();
        assert_eq!(second.slot, third.slot);
        assert_ne!(second.generation, third.generation);
        assert!(assets.get(second).is_err());
        let mut other = MeshAssets::new();
        let other_handle = other.insert(Mesh::cube(1.0).unwrap()).unwrap();
        assert!(other.get(third).is_err());
        assert!(assets.get(other_handle).is_err());
        drop(assets);
        assert!(MeshAssets::new().get(third).is_err());
        assert_eq!(retained.positions()[0].z, 1.0);
    }
    #[test]
    fn exhausted_generation_is_never_reused() {
        let mut assets = MeshAssets::new();
        let handle = assets.insert(Mesh::cube(1.0).unwrap()).unwrap();
        assets.slots[0].generation = u64::MAX;
        let exhausted = MeshHandle {
            generation: u64::MAX,
            ..handle
        };
        assert_eq!(
            assets.replace(exhausted, Mesh::floor(1.0).unwrap()),
            Err(MeshAssetError::GenerationExhausted)
        );
        assets.remove(exhausted).unwrap();
        let next = assets.insert(Mesh::floor(1.0).unwrap()).unwrap();
        assert_ne!(next.slot, exhausted.slot);
        assert!(assets.get(exhausted).is_err());
    }
}

//! Small, validated math boundary for right-handed 3D rendering.

/// Invalid or unrepresentable rendering math.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathError {
    NonFinite,
    ZeroLength,
    InvalidScale,
    InvalidProjection,
    Unrepresentable,
}

impl std::fmt::Display for MathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NonFinite => "3D math requires finite values",
            Self::ZeroLength => "rotation or direction must have nonzero length",
            Self::InvalidScale => "scale must be finite and positive on every axis",
            Self::InvalidProjection => {
                "perspective requires 0 < field of view < pi, positive aspect, and 0 < near < far"
            }
            Self::Unrepresentable => "3D math result is not representable as finite f32 values",
        })
    }
}

impl std::error::Error for MathError {}

/// A three-component vector. Constructors consuming vectors validate them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    pub const ONE: Self = Self::new(1.0, 1.0, 1.0);

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    pub fn normalized(self) -> Result<Self, MathError> {
        if !self.is_finite() {
            return Err(MathError::NonFinite);
        }
        normalize([self.x as f64, self.y as f64, self.z as f64])
    }
}

fn normalize(v: [f64; 3]) -> Result<Vec3, MathError> {
    let length = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if length == 0.0 {
        return Err(MathError::ZeroLength);
    }
    if !length.is_finite() {
        return Err(MathError::Unrepresentable);
    }
    Ok(Vec3::new(
        (v[0] / length) as f32,
        (v[1] / length) as f32,
        (v[2] / length) as f32,
    ))
}

/// A normalized quaternion, with vector part XYZ and scalar part W.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quaternion([f32; 4]);

impl Quaternion {
    pub const IDENTITY: Self = Self([0.0, 0.0, 0.0, 1.0]);

    /// Normalizes any finite, nonzero quaternion, including tiny inputs.
    pub fn new(x: f32, y: f32, z: f32, w: f32) -> Result<Self, MathError> {
        let values = [x, y, z, w];
        if !values.iter().all(|v| v.is_finite()) {
            return Err(MathError::NonFinite);
        }
        let length = values
            .iter()
            .map(|v| (*v as f64).powi(2))
            .sum::<f64>()
            .sqrt();
        if length == 0.0 {
            return Err(MathError::ZeroLength);
        }
        Ok(Self(values.map(|v| (v as f64 / length) as f32)))
    }

    pub fn components(self) -> [f32; 4] {
        self.0
    }

    fn rotation(self) -> [[f64; 3]; 3] {
        // Renormalize after the f32 storage round-trip.
        let norm = self
            .0
            .iter()
            .map(|v| (*v as f64).powi(2))
            .sum::<f64>()
            .sqrt();
        let [x, y, z, w] = self.0.map(|v| v as f64 / norm);
        [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y + z * w),
                2.0 * (x * z - y * w),
            ],
            [
                2.0 * (x * y - z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z + x * w),
            ],
            [
                2.0 * (x * z + y * w),
                2.0 * (y * z - x * w),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ]
    }
}

/// Finite column-major matrix acting on column vectors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4([[f32; 4]; 4]);

impl Mat4 {
    pub const IDENTITY: Self = Self([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);

    /// Returns columns, suitable for a column-major GPU matrix upload.
    pub fn columns(self) -> [[f32; 4]; 4] {
        self.0
    }

    fn from_f64(columns: [[f64; 4]; 4]) -> Result<Self, MathError> {
        let values = columns.map(|column| column.map(|v| v as f32));
        if !values.iter().flatten().all(|v| v.is_finite()) {
            return Err(MathError::Unrepresentable);
        }
        Ok(Self(values))
    }

    /// Multiplies a homogeneous column vector; does not divide by W.
    pub fn transform(self, vector: [f32; 4]) -> Result<[f32; 4], MathError> {
        if !vector.iter().all(|v| v.is_finite()) {
            return Err(MathError::NonFinite);
        }
        let result = std::array::from_fn(|row| {
            (0..4)
                .map(|col| self.0[col][row] as f64 * vector[col] as f64)
                .sum::<f64>() as f32
        });
        if result.iter().all(|v| v.is_finite()) {
            Ok(result)
        } else {
            Err(MathError::Unrepresentable)
        }
    }

    /// Computes `self × rhs`, applying `rhs` first.
    pub fn checked_mul(self, rhs: Self) -> Result<Self, MathError> {
        Self::from_f64(std::array::from_fn(|col| {
            std::array::from_fn(|row| {
                (0..4)
                    .map(|k| self.0[k][row] as f64 * rhs.0[col][k] as f64)
                    .sum()
            })
        }))
    }
}

/// Local translation × rotation × positive scale, without a parent hierarchy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform3d {
    translation: Vec3,
    rotation: Quaternion,
    scale: Vec3,
    matrix: Mat4,
}

impl Transform3d {
    pub fn new(translation: Vec3, rotation: Quaternion, scale: Vec3) -> Result<Self, MathError> {
        if !translation.is_finite() {
            return Err(MathError::NonFinite);
        }
        if !scale.is_finite() || scale.x <= 0.0 || scale.y <= 0.0 || scale.z <= 0.0 {
            return Err(MathError::InvalidScale);
        }
        let r = rotation.rotation();
        let s = [scale.x as f64, scale.y as f64, scale.z as f64];
        let mut columns = [[0.0; 4]; 4];
        for col in 0..3 {
            for row in 0..3 {
                columns[col][row] = r[col][row] * s[col];
            }
        }
        columns[3] = [
            translation.x as f64,
            translation.y as f64,
            translation.z as f64,
            1.0,
        ];
        let matrix = Mat4::from_f64(columns)?;
        Ok(Self {
            translation,
            rotation,
            scale,
            matrix,
        })
    }

    pub const fn identity() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quaternion::IDENTITY,
            scale: Vec3::ONE,
            matrix: Mat4::IDENTITY,
        }
    }
    pub fn translation(self) -> Vec3 {
        self.translation
    }
    pub fn rotation(self) -> Quaternion {
        self.rotation
    }
    pub fn scale(self) -> Vec3 {
        self.scale
    }
    pub fn matrix(self) -> Mat4 {
        self.matrix
    }

    /// Applies the inverse-transpose linear transform and normalizes the result.
    pub fn transform_normal(self, normal: Vec3) -> Result<Vec3, MathError> {
        if !normal.is_finite() {
            return Err(MathError::NonFinite);
        }
        let local = [
            normal.x as f64 / self.scale.x as f64,
            normal.y as f64 / self.scale.y as f64,
            normal.z as f64 / self.scale.z as f64,
        ];
        let r = self.rotation.rotation();
        normalize(std::array::from_fn(|row| {
            (0..3).map(|col| r[col][row] * local[col]).sum()
        }))
    }
}

impl Default for Transform3d {
    fn default() -> Self {
        Self::identity()
    }
}

/// Perspective camera: +Y up, local forward -Z, depth 0 at near and 1 at far.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerspectiveCamera {
    position: Vec3,
    rotation: Quaternion,
    vertical_fov_radians: f32,
    aspect: f32,
    near: f32,
    far: f32,
    view: Mat4,
    projection: Mat4,
}

impl PerspectiveCamera {
    /// Rotation maps camera-local axes into world space; camera scale is absent.
    pub fn new(
        position: Vec3,
        rotation: Quaternion,
        vertical_fov_radians: f32,
        aspect: f32,
        near: f32,
        far: f32,
    ) -> Result<Self, MathError> {
        if !position.is_finite() {
            return Err(MathError::NonFinite);
        }
        if ![vertical_fov_radians, aspect, near, far]
            .iter()
            .all(|v| v.is_finite())
            || vertical_fov_radians <= 0.0
            || vertical_fov_radians >= std::f32::consts::PI
            || aspect <= 0.0
            || near <= 0.0
            || far <= near
        {
            return Err(MathError::InvalidProjection);
        }
        let r = rotation.rotation();
        let p = [position.x as f64, position.y as f64, position.z as f64];
        let mut columns = [[0.0; 4]; 4];
        for col in 0..3 {
            for row in 0..3 {
                columns[col][row] = r[row][col];
            }
        }
        for row in 0..3 {
            columns[3][row] = -(0..3).map(|k| r[row][k] * p[k]).sum::<f64>();
        }
        columns[3][3] = 1.0;
        let view = Mat4::from_f64(columns)?;
        let f = 1.0 / (vertical_fov_radians as f64 / 2.0).tan();
        let depth = far as f64 / (near as f64 - far as f64);
        let projection = Mat4::from_f64([
            [f / aspect as f64, 0.0, 0.0, 0.0],
            [0.0, f, 0.0, 0.0],
            [0.0, 0.0, depth, -1.0],
            [0.0, 0.0, near as f64 * depth, 0.0],
        ])?;
        if projection.0[0][0] == 0.0 || projection.0[1][1] == 0.0 || projection.0[3][2] == 0.0 {
            return Err(MathError::Unrepresentable);
        }
        // Ensure the combined matrix can be uploaded without overflow as well.
        projection.checked_mul(view)?;
        Ok(Self {
            position,
            rotation,
            vertical_fov_radians,
            aspect,
            near,
            far,
            view,
            projection,
        })
    }
    pub fn position(self) -> Vec3 {
        self.position
    }
    pub fn rotation(self) -> Quaternion {
        self.rotation
    }
    pub fn vertical_fov_radians(self) -> f32 {
        self.vertical_fov_radians
    }
    pub fn aspect(self) -> f32 {
        self.aspect
    }
    pub fn near(self) -> f32 {
        self.near
    }
    pub fn far(self) -> f32 {
        self.far
    }
    pub fn view_matrix(self) -> Mat4 {
        self.view
    }
    pub fn projection_matrix(self) -> Mat4 {
        self.projection
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 2e-5, "{a} != {b}");
    }
    fn quarter_y() -> Quaternion {
        Quaternion::new(0.0, 1.0, 0.0, 1.0).unwrap()
    }
    fn camera() -> PerspectiveCamera {
        PerspectiveCamera::new(
            Vec3::ZERO,
            Quaternion::IDENTITY,
            std::f32::consts::FRAC_PI_2,
            2.0,
            1.0,
            10.0,
        )
        .unwrap()
    }

    #[test]
    fn translation_rotation_scale_order_and_handedness() {
        let t = Transform3d::new(
            Vec3::new(10.0, 20.0, 30.0),
            quarter_y(),
            Vec3::new(2.0, 3.0, 4.0),
        )
        .unwrap();
        let p = t.matrix().transform([1.0, 1.0, 1.0, 1.0]).unwrap();
        for (a, b) in p.into_iter().zip([14.0, 23.0, 28.0, 1.0]) {
            close(a, b);
        }
        assert_eq!(Transform3d::identity().matrix(), Mat4::IDENTITY);
    }

    #[test]
    fn normals_use_inverse_transpose_and_remain_perpendicular() {
        let t = Transform3d::new(Vec3::ONE, quarter_y(), Vec3::new(2.0, 1.0, 4.0)).unwrap();
        let n = t.transform_normal(Vec3::new(1.0, 1.0, 0.0)).unwrap();
        close(n.x, 0.0);
        close(n.y, 2.0 / 5.0f32.sqrt());
        close(n.z, -1.0 / 5.0f32.sqrt());
        let tangent = t.matrix().transform([1.0, -1.0, 0.0, 0.0]).unwrap();
        close(n.x * tangent[0] + n.y * tangent[1] + n.z * tangent[2], 0.0);
        assert_eq!(t.transform_normal(Vec3::ZERO), Err(MathError::ZeroLength));
    }

    #[test]
    fn perspective_maps_near_far_and_vertical_fov() {
        let p = camera().projection_matrix();
        for (z, expected) in [(-1.0, 0.0), (-10.0, 1.0)] {
            let v = p.transform([0.0, 0.0, z, 1.0]).unwrap();
            close(v[2] / v[3], expected);
        }
        let edge = p.transform([2.0, 1.0, -1.0, 1.0]).unwrap();
        close(edge[0] / edge[3], 1.0);
        close(edge[1] / edge[3], 1.0);
        let near = p.transform([1.0, 0.0, -2.0, 1.0]).unwrap();
        let far = p.transform([1.0, 0.0, -4.0, 1.0]).unwrap();
        close(near[0] / near[3], 2.0 * far[0] / far[3]);
    }

    #[test]
    fn camera_view_is_inverse_pose_and_composition_uses_columns() {
        let position = Vec3::new(3.0, 4.0, 5.0);
        let c = PerspectiveCamera::new(position, quarter_y(), 1.0, 1.5, 0.1, 100.0).unwrap();
        let pose = Transform3d::new(position, quarter_y(), Vec3::ONE)
            .unwrap()
            .matrix();
        let identity = c.view_matrix().checked_mul(pose).unwrap();
        for (actual, expected) in identity
            .columns()
            .into_iter()
            .flatten()
            .zip(Mat4::IDENTITY.columns().into_iter().flatten())
        {
            close(actual, expected);
        }
        let forward_world = [2.0, 4.0, 5.0, 1.0];
        let local = c.view_matrix().transform(forward_world).unwrap();
        close(local[0], 0.0);
        close(local[1], 0.0);
        close(local[2], -1.0);
    }

    #[test]
    fn invalid_and_extreme_inputs_are_handled_without_nan() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(Quaternion::new(bad, 0.0, 0.0, 1.0).is_err());
            assert!(
                Transform3d::new(Vec3::new(bad, 0.0, 0.0), Quaternion::IDENTITY, Vec3::ONE)
                    .is_err()
            );
            assert!(Vec3::new(bad, 0.0, 0.0).normalized().is_err());
        }
        assert!(Quaternion::new(0.0, 0.0, 0.0, 0.0).is_err());
        for n in [f32::from_bits(1), f32::MAX] {
            close(
                Quaternion::new(n, 0.0, 0.0, 0.0).unwrap().components()[0],
                1.0,
            );
            close(Vec3::new(n, 0.0, 0.0).normalized().unwrap().x, 1.0);
        }
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(
                Transform3d::new(Vec3::ZERO, Quaternion::IDENTITY, Vec3::new(scale, 1.0, 1.0))
                    .is_err()
            );
        }
        let tiny = Transform3d::new(
            Vec3::ZERO,
            Quaternion::IDENTITY,
            Vec3::new(f32::from_bits(1), 1.0, 1.0),
        )
        .unwrap();
        assert_eq!(
            tiny.transform_normal(Vec3::new(f32::MAX, 0.0, 0.0))
                .unwrap(),
            Vec3::new(1.0, 0.0, 0.0)
        );
        let huge = Transform3d::new(
            Vec3::ZERO,
            Quaternion::IDENTITY,
            Vec3::new(f32::MAX, 1.0, 1.0),
        )
        .unwrap()
        .matrix();
        assert!(huge.transform([2.0, 0.0, 0.0, 1.0]).is_err());
        assert!(huge.checked_mul(huge).is_err());
    }

    #[test]
    fn rejects_invalid_or_unrepresentable_projections() {
        for [fov, aspect, near, far] in [
            [0.0, 1.0, 0.1, 10.0],
            [std::f32::consts::PI, 1.0, 0.1, 10.0],
            [1.0, 0.0, 0.1, 10.0],
            [1.0, -1.0, 0.1, 10.0],
            [1.0, 1.0, 0.0, 10.0],
            [1.0, 1.0, 10.0, 10.0],
            [1.0, 1.0, 11.0, 10.0],
            [f32::NAN, 1.0, 0.1, 10.0],
            [1.0, f32::INFINITY, 0.1, 10.0],
            [1.0, 1.0, 0.1, f32::INFINITY],
            [f32::from_bits(1), 1.0, 0.1, 10.0],
        ] {
            assert!(
                PerspectiveCamera::new(Vec3::ZERO, Quaternion::IDENTITY, fov, aspect, near, far)
                    .is_err()
            );
        }
    }
}

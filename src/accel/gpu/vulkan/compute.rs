pub fn squared_l2_distance(left: &[f32], right: &[f32]) -> Option<f32> {
    crate::accel::gpu::kernels::squared_l2_distance(left, right)
}

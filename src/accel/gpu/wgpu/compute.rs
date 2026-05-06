use crate::accel::gpu::kernels;

pub fn squared_l2_distance(left: &[f32], right: &[f32]) -> Option<f32> {
    kernels::squared_l2_distance(left, right)
}

pub fn inner_product(left: &[f32], right: &[f32]) -> Option<f32> {
    kernels::inner_product(left, right)
}

pub fn cosine_distance(left: &[f32], right: &[f32]) -> Option<f32> {
    kernels::cosine_distance(left, right)
}

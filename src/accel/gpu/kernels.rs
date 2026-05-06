pub fn squared_l2_distance(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.len() != right.len() {
        return None;
    }

    Some(
        left.iter()
            .zip(right)
            .map(|(a, b)| {
                let delta = a - b;
                delta * delta
            })
            .sum(),
    )
}

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

pub fn inner_product(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.len() != right.len() {
        return None;
    }
    Some(left.iter().zip(right).map(|(a, b)| a * b).sum())
}

pub fn cosine_distance(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.len() != right.len() {
        return None;
    }
    let mut dot = 0.0f32;
    let mut norm_l = 0.0f32;
    let mut norm_r = 0.0f32;
    for (a, b) in left.iter().zip(right) {
        dot += a * b;
        norm_l += a * a;
        norm_r += b * b;
    }
    let denom = norm_l.sqrt() * norm_r.sqrt();
    if denom < f32::EPSILON {
        return Some(1.0);
    }
    Some(1.0 - (dot / denom))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l2_basic() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![4.0, 5.0, 6.0];
        let d = squared_l2_distance(&a, &b).unwrap();
        assert!((d - 27.0).abs() < 1e-4);
    }

    #[test]
    fn inner_product_basic() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![4.0, 5.0, 6.0];
        let ip = inner_product(&a, &b).unwrap();
        assert!((ip - 32.0).abs() < 1e-4);
    }

    #[test]
    fn cosine_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let d = cosine_distance(&a, &a).unwrap();
        assert!(d.abs() < 1e-4);
    }

    #[test]
    fn cosine_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let d = cosine_distance(&a, &b).unwrap();
        assert!((d - 1.0).abs() < 1e-4);
    }

    #[test]
    fn length_mismatch() {
        assert!(squared_l2_distance(&[1.0], &[1.0, 2.0]).is_none());
        assert!(inner_product(&[1.0], &[1.0, 2.0]).is_none());
        assert!(cosine_distance(&[1.0], &[1.0, 2.0]).is_none());
    }
}

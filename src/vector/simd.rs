#[inline]
pub(crate) fn dot(query: &[f32], target: &[f32]) -> f64 {
    if query.len() != target.len() || query.is_empty() {
        return 0.0;
    }

    #[cfg(target_arch = "x86_64")]
    {
        if query.len() >= 8 {
            return unsafe { dot_x86(query, target) };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if query.len() >= 8 {
            return unsafe { dot_aarch64(query, target) };
        }
    }

    scalar_dot(query, target)
}

#[inline]
pub(crate) fn squared_l2(query: &[f32], target: &[f32]) -> f64 {
    if query.len() != target.len() {
        return f64::MAX;
    }
    if query.is_empty() {
        return 0.0;
    }

    #[cfg(target_arch = "x86_64")]
    {
        if query.len() >= 8 {
            return unsafe { squared_l2_x86(query, target) };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if query.len() >= 8 {
            return unsafe { squared_l2_aarch64(query, target) };
        }
    }

    scalar_squared_l2(query, target)
}

#[inline]
pub(crate) fn cosine_components(query: &[f32], target: &[f32]) -> (f64, f64, f64) {
    if query.len() != target.len() || query.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    #[cfg(target_arch = "x86_64")]
    {
        if query.len() >= 8 {
            return unsafe { cosine_components_x86(query, target) };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        if query.len() >= 8 {
            return unsafe { cosine_components_aarch64(query, target) };
        }
    }

    scalar_cosine_components(query, target)
}

#[inline]
fn scalar_dot(query: &[f32], target: &[f32]) -> f64 {
    query
        .iter()
        .zip(target.iter())
        .map(|(q, t)| f64::from(*q) * f64::from(*t))
        .sum()
}

#[inline]
fn scalar_squared_l2(query: &[f32], target: &[f32]) -> f64 {
    query
        .iter()
        .zip(target.iter())
        .map(|(q, t)| {
            let diff = f64::from(*q) - f64::from(*t);
            diff * diff
        })
        .sum()
}

#[inline]
fn scalar_cosine_components(query: &[f32], target: &[f32]) -> (f64, f64, f64) {
    let mut dot = 0f64;
    let mut qnorm = 0f64;
    let mut tnorm = 0f64;
    for (q, t) in query.iter().zip(target.iter()) {
        let qv = f64::from(*q);
        let tv = f64::from(*t);
        dot += qv * tv;
        qnorm += qv * qv;
        tnorm += tv * tv;
    }
    (dot, qnorm, tnorm)
}

#[cfg(target_arch = "x86_64")]
unsafe fn dot_x86(query: &[f32], target: &[f32]) -> f64 {
    use std::arch::x86_64::*;

    let mut acc = _mm_setzero_ps();
    let mut index = 0usize;
    while index + 4 <= query.len() {
        let q = _mm_loadu_ps(query.as_ptr().add(index));
        let t = _mm_loadu_ps(target.as_ptr().add(index));
        acc = _mm_add_ps(acc, _mm_mul_ps(q, t));
        index += 4;
    }

    let mut lanes = [0f32; 4];
    _mm_storeu_ps(lanes.as_mut_ptr(), acc);
    let mut sum = lanes.iter().map(|value| *value as f64).sum::<f64>();
    while index < query.len() {
        sum += query[index] as f64 * target[index] as f64;
        index += 1;
    }

    sum
}

#[cfg(target_arch = "x86_64")]
unsafe fn squared_l2_x86(query: &[f32], target: &[f32]) -> f64 {
    use std::arch::x86_64::*;

    let mut acc = _mm_setzero_ps();
    let mut index = 0usize;
    while index + 4 <= query.len() {
        let q = _mm_loadu_ps(query.as_ptr().add(index));
        let t = _mm_loadu_ps(target.as_ptr().add(index));
        let diff = _mm_sub_ps(q, t);
        acc = _mm_add_ps(acc, _mm_mul_ps(diff, diff));
        index += 4;
    }

    let mut lanes = [0f32; 4];
    _mm_storeu_ps(lanes.as_mut_ptr(), acc);
    let mut sum = lanes.iter().map(|value| *value as f64).sum::<f64>();
    while index < query.len() {
        let diff = query[index] as f64 - target[index] as f64;
        sum += diff * diff;
        index += 1;
    }

    sum
}

#[cfg(target_arch = "x86_64")]
unsafe fn cosine_components_x86(query: &[f32], target: &[f32]) -> (f64, f64, f64) {
    use std::arch::x86_64::*;

    let mut dot_acc = _mm_setzero_ps();
    let mut qnorm_acc = _mm_setzero_ps();
    let mut tnorm_acc = _mm_setzero_ps();
    let mut index = 0usize;
    while index + 4 <= query.len() {
        let q = _mm_loadu_ps(query.as_ptr().add(index));
        let t = _mm_loadu_ps(target.as_ptr().add(index));
        dot_acc = _mm_add_ps(dot_acc, _mm_mul_ps(q, t));
        qnorm_acc = _mm_add_ps(qnorm_acc, _mm_mul_ps(q, q));
        tnorm_acc = _mm_add_ps(tnorm_acc, _mm_mul_ps(t, t));
        index += 4;
    }

    let mut dot_lanes = [0f32; 4];
    let mut qnorm_lanes = [0f32; 4];
    let mut tnorm_lanes = [0f32; 4];
    _mm_storeu_ps(dot_lanes.as_mut_ptr(), dot_acc);
    _mm_storeu_ps(qnorm_lanes.as_mut_ptr(), qnorm_acc);
    _mm_storeu_ps(tnorm_lanes.as_mut_ptr(), tnorm_acc);
    let mut dot = dot_lanes.iter().map(|value| *value as f64).sum::<f64>();
    let mut qnorm = qnorm_lanes.iter().map(|value| *value as f64).sum::<f64>();
    let mut tnorm = tnorm_lanes.iter().map(|value| *value as f64).sum::<f64>();

    while index < query.len() {
        let qv = query[index] as f64;
        let tv = target[index] as f64;
        dot += qv * tv;
        qnorm += qv * qv;
        tnorm += tv * tv;
        index += 1;
    }

    (dot, qnorm, tnorm)
}

#[cfg(target_arch = "aarch64")]
unsafe fn dot_aarch64(query: &[f32], target: &[f32]) -> f64 {
    use std::arch::aarch64::{vdupq_n_f32, vld1q_f32, vaddq_f32, vmulq_f32, vaddvq_f32};

    let mut acc = vdupq_n_f32(0.0);
    let mut index = 0usize;
    while index + 4 <= query.len() {
        let q = vld1q_f32(query.as_ptr().add(index));
        let t = vld1q_f32(target.as_ptr().add(index));
        acc = vaddq_f32(acc, vmulq_f32(q, t));
        index += 4;
    }

    let mut sum = f64::from(vaddvq_f32(acc));
    while index < query.len() {
        sum += f64::from(query[index]) * f64::from(target[index]);
        index += 1;
    }

    sum
}

#[cfg(target_arch = "aarch64")]
unsafe fn squared_l2_aarch64(query: &[f32], target: &[f32]) -> f64 {
    use std::arch::aarch64::{vdupq_n_f32, vld1q_f32, vsubq_f32, vaddq_f32, vmulq_f32, vaddvq_f32};

    let mut acc = vdupq_n_f32(0.0);
    let mut index = 0usize;
    while index + 4 <= query.len() {
        let q = vld1q_f32(query.as_ptr().add(index));
        let t = vld1q_f32(target.as_ptr().add(index));
        let diff = vsubq_f32(q, t);
        acc = vaddq_f32(acc, vmulq_f32(diff, diff));
        index += 4;
    }

    let mut sum = f64::from(vaddvq_f32(acc));
    while index < query.len() {
        let diff = f64::from(query[index]) - f64::from(target[index]);
        sum += diff * diff;
        index += 1;
    }

    sum
}

#[cfg(target_arch = "aarch64")]
unsafe fn cosine_components_aarch64(query: &[f32], target: &[f32]) -> (f64, f64, f64) {
    use std::arch::aarch64::{vdupq_n_f32, vld1q_f32, vaddq_f32, vmulq_f32, vaddvq_f32};

    let mut dot_acc = vdupq_n_f32(0.0);
    let mut qnorm_acc = vdupq_n_f32(0.0);
    let mut tnorm_acc = vdupq_n_f32(0.0);
    let mut index = 0usize;
    while index + 4 <= query.len() {
        let q = vld1q_f32(query.as_ptr().add(index));
        let t = vld1q_f32(target.as_ptr().add(index));
        dot_acc = vaddq_f32(dot_acc, vmulq_f32(q, t));
        qnorm_acc = vaddq_f32(qnorm_acc, vmulq_f32(q, q));
        tnorm_acc = vaddq_f32(tnorm_acc, vmulq_f32(t, t));
        index += 4;
    }

    let mut dot = f64::from(vaddvq_f32(dot_acc));
    let mut qnorm = f64::from(vaddvq_f32(qnorm_acc));
    let mut tnorm = f64::from(vaddvq_f32(tnorm_acc));
    while index < query.len() {
        let qv = f64::from(query[index]);
        let tv = f64::from(target[index]);
        dot += qv * tv;
        qnorm += qv * qv;
        tnorm += tv * tv;
        index += 1;
    }

    (dot, qnorm, tnorm)
}

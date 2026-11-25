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
    use std::arch::x86_64::{
        _mm_add_pd, _mm_cvtps_pd, _mm_loadu_ps, _mm_movehl_ps, _mm_mul_pd, _mm_setzero_pd,
        _mm_storeu_pd,
    };

    let mut acc = _mm_setzero_pd();
    let mut index = 0usize;
    while index + 4 <= query.len() {
        let q = _mm_loadu_ps(query.as_ptr().add(index));
        let t = _mm_loadu_ps(target.as_ptr().add(index));
        let q_low = _mm_cvtps_pd(q);
        let t_low = _mm_cvtps_pd(t);
        let q_high = _mm_cvtps_pd(_mm_movehl_ps(q, q));
        let t_high = _mm_cvtps_pd(_mm_movehl_ps(t, t));
        acc = _mm_add_pd(acc, _mm_mul_pd(q_low, t_low));
        acc = _mm_add_pd(acc, _mm_mul_pd(q_high, t_high));
        index += 4;
    }

    let mut lanes = [0f64; 2];
    _mm_storeu_pd(lanes.as_mut_ptr(), acc);
    let mut sum = lanes.iter().sum::<f64>();
    while index < query.len() {
        sum += f64::from(query[index]) * f64::from(target[index]);
        index += 1;
    }

    sum
}

#[cfg(target_arch = "x86_64")]
unsafe fn squared_l2_x86(query: &[f32], target: &[f32]) -> f64 {
    use std::arch::x86_64::{
        _mm_add_pd, _mm_cvtps_pd, _mm_loadu_ps, _mm_movehl_ps, _mm_mul_pd, _mm_setzero_pd,
        _mm_storeu_pd, _mm_sub_pd,
    };

    let mut acc = _mm_setzero_pd();
    let mut index = 0usize;
    while index + 4 <= query.len() {
        let q = _mm_loadu_ps(query.as_ptr().add(index));
        let t = _mm_loadu_ps(target.as_ptr().add(index));
        let q_low = _mm_cvtps_pd(q);
        let t_low = _mm_cvtps_pd(t);
        let q_high = _mm_cvtps_pd(_mm_movehl_ps(q, q));
        let t_high = _mm_cvtps_pd(_mm_movehl_ps(t, t));
        let diff_low = _mm_sub_pd(q_low, t_low);
        let diff_high = _mm_sub_pd(q_high, t_high);
        acc = _mm_add_pd(acc, _mm_mul_pd(diff_low, diff_low));
        acc = _mm_add_pd(acc, _mm_mul_pd(diff_high, diff_high));
        index += 4;
    }

    let mut lanes = [0f64; 2];
    _mm_storeu_pd(lanes.as_mut_ptr(), acc);
    let mut sum = lanes.iter().sum::<f64>();
    while index < query.len() {
        let diff = f64::from(query[index]) - f64::from(target[index]);
        sum += diff * diff;
        index += 1;
    }

    sum
}

#[cfg(target_arch = "x86_64")]
unsafe fn cosine_components_x86(query: &[f32], target: &[f32]) -> (f64, f64, f64) {
    use std::arch::x86_64::{
        _mm_add_pd, _mm_cvtps_pd, _mm_loadu_ps, _mm_movehl_ps, _mm_mul_pd, _mm_setzero_pd,
        _mm_storeu_pd,
    };

    let mut dot_acc = _mm_setzero_pd();
    let mut qnorm_acc = _mm_setzero_pd();
    let mut tnorm_acc = _mm_setzero_pd();
    let mut index = 0usize;
    while index + 4 <= query.len() {
        let q = _mm_loadu_ps(query.as_ptr().add(index));
        let t = _mm_loadu_ps(target.as_ptr().add(index));
        let q_low = _mm_cvtps_pd(q);
        let t_low = _mm_cvtps_pd(t);
        let q_high = _mm_cvtps_pd(_mm_movehl_ps(q, q));
        let t_high = _mm_cvtps_pd(_mm_movehl_ps(t, t));
        dot_acc = _mm_add_pd(dot_acc, _mm_mul_pd(q_low, t_low));
        dot_acc = _mm_add_pd(dot_acc, _mm_mul_pd(q_high, t_high));
        qnorm_acc = _mm_add_pd(qnorm_acc, _mm_mul_pd(q_low, q_low));
        qnorm_acc = _mm_add_pd(qnorm_acc, _mm_mul_pd(q_high, q_high));
        tnorm_acc = _mm_add_pd(tnorm_acc, _mm_mul_pd(t_low, t_low));
        tnorm_acc = _mm_add_pd(tnorm_acc, _mm_mul_pd(t_high, t_high));
        index += 4;
    }

    let mut dot_lanes = [0f64; 2];
    let mut qnorm_lanes = [0f64; 2];
    let mut tnorm_lanes = [0f64; 2];
    _mm_storeu_pd(dot_lanes.as_mut_ptr(), dot_acc);
    _mm_storeu_pd(qnorm_lanes.as_mut_ptr(), qnorm_acc);
    _mm_storeu_pd(tnorm_lanes.as_mut_ptr(), tnorm_acc);
    let mut dot = dot_lanes.iter().sum::<f64>();
    let mut qnorm = qnorm_lanes.iter().sum::<f64>();
    let mut tnorm = tnorm_lanes.iter().sum::<f64>();

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

#[cfg(target_arch = "aarch64")]
unsafe fn dot_aarch64(query: &[f32], target: &[f32]) -> f64 {
    use std::arch::aarch64::{
        vaddq_f64, vaddvq_f64, vcvt_f64_f32, vcvt_high_f64_f32, vdupq_n_f64, vget_low_f32,
        vld1q_f32, vmulq_f64,
    };

    let mut acc = vdupq_n_f64(0.0);
    let mut index = 0usize;
    while index + 4 <= query.len() {
        let q = vld1q_f32(query.as_ptr().add(index));
        let t = vld1q_f32(target.as_ptr().add(index));
        let q_low = vcvt_f64_f32(vget_low_f32(q));
        let t_low = vcvt_f64_f32(vget_low_f32(t));
        let q_high = vcvt_high_f64_f32(q);
        let t_high = vcvt_high_f64_f32(t);
        acc = vaddq_f64(acc, vmulq_f64(q_low, t_low));
        acc = vaddq_f64(acc, vmulq_f64(q_high, t_high));
        index += 4;
    }

    let mut sum = vaddvq_f64(acc);
    while index < query.len() {
        sum += f64::from(query[index]) * f64::from(target[index]);
        index += 1;
    }

    sum
}

#[cfg(target_arch = "aarch64")]
unsafe fn squared_l2_aarch64(query: &[f32], target: &[f32]) -> f64 {
    use std::arch::aarch64::{
        vaddq_f64, vaddvq_f64, vcvt_f64_f32, vcvt_high_f64_f32, vdupq_n_f64, vget_low_f32,
        vld1q_f32, vmulq_f64, vsubq_f64,
    };

    let mut acc = vdupq_n_f64(0.0);
    let mut index = 0usize;
    while index + 4 <= query.len() {
        let q = vld1q_f32(query.as_ptr().add(index));
        let t = vld1q_f32(target.as_ptr().add(index));
        let q_low = vcvt_f64_f32(vget_low_f32(q));
        let t_low = vcvt_f64_f32(vget_low_f32(t));
        let q_high = vcvt_high_f64_f32(q);
        let t_high = vcvt_high_f64_f32(t);
        let diff_low = vsubq_f64(q_low, t_low);
        let diff_high = vsubq_f64(q_high, t_high);
        acc = vaddq_f64(acc, vmulq_f64(diff_low, diff_low));
        acc = vaddq_f64(acc, vmulq_f64(diff_high, diff_high));
        index += 4;
    }

    let mut sum = vaddvq_f64(acc);
    while index < query.len() {
        let diff = f64::from(query[index]) - f64::from(target[index]);
        sum += diff * diff;
        index += 1;
    }

    sum
}

#[cfg(target_arch = "aarch64")]
unsafe fn cosine_components_aarch64(query: &[f32], target: &[f32]) -> (f64, f64, f64) {
    use std::arch::aarch64::{
        vaddq_f64, vaddvq_f64, vcvt_f64_f32, vcvt_high_f64_f32, vdupq_n_f64, vget_low_f32,
        vld1q_f32, vmulq_f64,
    };

    let mut dot_acc = vdupq_n_f64(0.0);
    let mut qnorm_acc = vdupq_n_f64(0.0);
    let mut tnorm_acc = vdupq_n_f64(0.0);
    let mut index = 0usize;
    while index + 4 <= query.len() {
        let q = vld1q_f32(query.as_ptr().add(index));
        let t = vld1q_f32(target.as_ptr().add(index));
        let q_low = vcvt_f64_f32(vget_low_f32(q));
        let t_low = vcvt_f64_f32(vget_low_f32(t));
        let q_high = vcvt_high_f64_f32(q);
        let t_high = vcvt_high_f64_f32(t);
        dot_acc = vaddq_f64(dot_acc, vmulq_f64(q_low, t_low));
        dot_acc = vaddq_f64(dot_acc, vmulq_f64(q_high, t_high));
        qnorm_acc = vaddq_f64(qnorm_acc, vmulq_f64(q_low, q_low));
        qnorm_acc = vaddq_f64(qnorm_acc, vmulq_f64(q_high, q_high));
        tnorm_acc = vaddq_f64(tnorm_acc, vmulq_f64(t_low, t_low));
        tnorm_acc = vaddq_f64(tnorm_acc, vmulq_f64(t_high, t_high));
        index += 4;
    }

    let mut dot = vaddvq_f64(dot_acc);
    let mut qnorm = vaddvq_f64(qnorm_acc);
    let mut tnorm = vaddvq_f64(tnorm_acc);
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

//! Mathematical functions for knowledge distillation

use super::config::Temperature;

/// Computes soft targets from teacher model logits
#[must_use]
pub fn soft_targets(logits: &[f32], temperature: Temperature) -> Vec<f32> {
    let scaled_logits = temperature.scale_logits(logits);
    softmax(&scaled_logits)
}

/// Softmax activation function with numerical stability
#[must_use]
pub fn softmax(logits: &[f32]) -> Vec<f32> {
    if logits.is_empty() {
        return Vec::new();
    }

    let max_logit = logits.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));

    let exp_sum: f32 = logits.iter().map(|&x| (x - max_logit).exp()).sum();

    if exp_sum == 0.0 {
        // Uniform distribution as fallback
        let uniform = 1.0 / logits.len() as f32;
        return vec![uniform; logits.len()];
    }

    logits
        .iter()
        .map(|&x| (x - max_logit).exp() / exp_sum)
        .collect()
}

/// Log-softmax activation function with numerical stability
#[must_use]
pub fn log_softmax(logits: &[f32]) -> Vec<f32> {
    if logits.is_empty() {
        return Vec::new();
    }

    let max_logit = logits.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
    let exp_sum: f32 = logits.iter().map(|&x| (x - max_logit).exp()).sum();
    let log_sum_exp = max_logit + exp_sum.ln();

    logits.iter().map(|&x| x - log_sum_exp).collect()
}

/// Computes KL divergence between distributions: KL(P || Q)
/// P is the target (teacher), Q is the prediction (student)
#[must_use]
pub fn kl_divergence(p: &[f32], q: &[f32]) -> f32 {
    if p.len() != q.len() || p.is_empty() {
        return 0.0;
    }

    p.iter()
        .zip(q.iter())
        .map(|(&pi, &qi)| {
            if pi > 1e-10 && qi > 1e-10 {
                pi * (pi / qi).ln()
            } else if pi > 1e-10 {
                // q is near zero but p is not - large divergence
                pi * 20.0 // Cap at reasonable value
            } else {
                0.0
            }
        })
        .sum()
}

/// Computes KL divergence using log probabilities for numerical stability
#[must_use]
pub fn kl_divergence_from_logits(
    teacher_logits: &[f32],
    student_logits: &[f32],
    temperature: Temperature,
) -> f32 {
    let teacher_scaled = temperature.scale_logits(teacher_logits);
    let student_scaled = temperature.scale_logits(student_logits);

    let teacher_probs = softmax(&teacher_scaled);
    let student_log_probs = log_softmax(&student_scaled);

    // KL(teacher || student) = sum(teacher * log(teacher)) - sum(teacher * log(student))
    let teacher_entropy: f32 = teacher_probs
        .iter()
        .map(|&p| if p > 1e-10 { -p * p.ln() } else { 0.0 })
        .sum();

    let cross_entropy: f32 = teacher_probs
        .iter()
        .zip(student_log_probs.iter())
        .map(|(&p, &log_q)| -p * log_q)
        .sum();

    // Scale by T^2 as per Hinton et al.
    (cross_entropy - teacher_entropy) * temperature.0.powi(2)
}

/// Computes mean squared error between predictions
#[must_use]
pub fn mse_loss(pred: &[f32], target: &[f32]) -> f32 {
    if pred.len() != target.len() || pred.is_empty() {
        return 0.0;
    }

    let sum: f32 = pred
        .iter()
        .zip(target.iter())
        .map(|(&p, &t)| (p - t).powi(2))
        .sum();

    sum / pred.len() as f32
}

/// Computes cross-entropy loss
#[must_use]
pub fn cross_entropy_loss(pred: &[f32], target: &[f32]) -> f32 {
    if pred.len() != target.len() || pred.is_empty() {
        return 0.0;
    }

    pred.iter()
        .zip(target.iter())
        .map(|(&p, &t)| {
            if t > 1e-10 {
                -t * (p + 1e-10).ln()
            } else {
                0.0
            }
        })
        .sum()
}

/// Computes cross-entropy loss with class index (hard label)
#[must_use]
pub fn cross_entropy_with_label(pred_logits: &[f32], label: usize) -> f32 {
    if label >= pred_logits.len() {
        return f32::MAX;
    }

    let log_probs = log_softmax(pred_logits);
    -log_probs.get(label).copied().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_softmax() {
        let logits = vec![1.0, 2.0, 3.0];
        let probs = softmax(&logits);

        let sum: f32 = probs.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
        assert!(probs[0] < probs[1]);
        assert!(probs[1] < probs[2]);

        for &p in &probs {
            assert!(p > 0.0);
        }
    }

    #[test]
    fn test_softmax_numerical_stability() {
        let large_logits = vec![1000.0, 1001.0, 1002.0];
        let probs = softmax(&large_logits);

        let sum: f32 = probs.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5);

        for &p in &probs {
            assert!(p.is_finite());
        }
    }

    #[test]
    fn test_log_softmax() {
        let logits = vec![1.0, 2.0, 3.0];
        let log_probs = log_softmax(&logits);
        let probs = softmax(&logits);

        for (lp, p) in log_probs.iter().zip(probs.iter()) {
            assert!((lp - p.ln()).abs() < 1e-5);
        }
    }

    #[test]
    fn test_soft_targets() {
        let logits = vec![1.0, 2.0, 3.0];
        let temp = Temperature::new(2.0);
        let soft = soft_targets(&logits, temp);

        let sum: f32 = soft.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);

        let regular = softmax(&logits);
        let entropy_soft: f32 = soft
            .iter()
            .map(|&p| if p > 0.0 { -p * p.ln() } else { 0.0 })
            .sum();
        let entropy_regular: f32 = regular
            .iter()
            .map(|&p| if p > 0.0 { -p * p.ln() } else { 0.0 })
            .sum();
        assert!(entropy_soft > entropy_regular);
    }

    #[test]
    fn test_kl_divergence() {
        let p = vec![0.5, 0.3, 0.2];
        let q = vec![0.4, 0.4, 0.2];

        let kl = kl_divergence(&p, &q);
        assert!(kl > 0.0);

        let kl_same = kl_divergence(&p, &p);
        assert!(kl_same.abs() < 1e-6);
    }

    #[test]
    fn test_kl_divergence_from_logits() {
        let teacher = vec![1.0, 2.0, 3.0];
        let student = vec![0.9, 2.1, 2.8];
        let temp = Temperature::new(2.0);

        let kl = kl_divergence_from_logits(&teacher, &student, temp);
        assert!(kl.is_finite());
        assert!(kl >= 0.0);
    }

    #[test]
    fn test_mse_loss() {
        let pred = vec![1.0, 2.0, 3.0];
        let target = vec![1.1, 2.2, 2.9];

        let loss = mse_loss(&pred, &target);
        assert!(loss > 0.0);
        assert!(loss < 0.1);

        let zero_loss = mse_loss(&pred, &pred);
        assert!(zero_loss.abs() < 1e-6);
    }

    #[test]
    fn test_cross_entropy_loss() {
        let pred = vec![0.7, 0.2, 0.1];
        let target = vec![1.0, 0.0, 0.0];

        let loss = cross_entropy_loss(&pred, &target);
        assert!(loss > 0.0);
        assert!(loss.is_finite());
    }

    #[test]
    fn test_cross_entropy_with_label() {
        let logits = vec![1.0, 5.0, 2.0];

        let loss_correct = cross_entropy_with_label(&logits, 1);
        let loss_wrong = cross_entropy_with_label(&logits, 0);

        assert!(loss_correct < loss_wrong);
    }
}

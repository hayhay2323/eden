use std::collections::HashSet;
use std::hash::Hash;

use rust_decimal::Decimal;

/// Clamp a decimal to the closed unit interval [0, 1].
pub fn clamp_unit_interval(value: Decimal) -> Decimal {
    if value < Decimal::ZERO {
        Decimal::ZERO
    } else if value > Decimal::ONE {
        Decimal::ONE
    } else {
        value
    }
}

/// (A - B) / (A + B). Returns 0 when denominator is 0. Arithmetic identity, not a threshold.
pub fn normalized_ratio(a: Decimal, b: Decimal) -> Decimal {
    let sum = a + b;
    if sum == Decimal::ZERO {
        Decimal::ZERO
    } else {
        (a - b) / sum
    }
}

/// Cosine similarity of two N-dimensional vectors.
/// Returns 0 if either vector has zero magnitude.
pub fn cosine_similarity<const N: usize>(a: [Decimal; N], b: [Decimal; N]) -> Decimal {
    let dot: Decimal = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a_sq: Decimal = a.iter().map(|x| x * x).sum();
    let mag_b_sq: Decimal = b.iter().map(|x| x * x).sum();

    if mag_a_sq == Decimal::ZERO || mag_b_sq == Decimal::ZERO {
        return Decimal::ZERO;
    }

    // Use Newton's method for sqrt since rust_decimal has no built-in sqrt.
    let mag_a = decimal_sqrt(mag_a_sq);
    let mag_b = decimal_sqrt(mag_b_sq);
    let denom = mag_a * mag_b;

    if denom == Decimal::ZERO {
        Decimal::ZERO
    } else {
        dot / denom
    }
}

/// Jaccard coefficient: |A ∩ B| / |A ∪ B|. Returns 0 if both sets are empty.
pub fn jaccard<T: Eq + Hash>(a: &HashSet<T>, b: &HashSet<T>) -> Decimal {
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(intersection as i64) / Decimal::from(union as i64)
    }
}

/// Newton's method square root for Decimal. Converges to 28 decimal places.
pub fn decimal_sqrt(x: Decimal) -> Decimal {
    if x <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    // Initial guess: start with 1 and iterate.
    let mut guess = Decimal::ONE;
    let two = Decimal::from(2);
    for _ in 0..50 {
        let next = (guess + x / guess) / two;
        if next == guess {
            break;
        }
        guess = next;
    }
    guess
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // ── normalized_ratio ──

    #[test]
    fn normalized_ratio_basic() {
        assert_eq!(normalized_ratio(dec!(3), dec!(1)), dec!(0.5));
    }

    #[test]
    fn clamp_unit_interval_bounds_values() {
        assert_eq!(clamp_unit_interval(dec!(-0.2)), Decimal::ZERO);
        assert_eq!(clamp_unit_interval(dec!(0.4)), dec!(0.4));
        assert_eq!(clamp_unit_interval(dec!(1.4)), Decimal::ONE);
    }

    #[test]
    fn normalized_ratio_zero_denominator() {
        assert_eq!(normalized_ratio(dec!(0), dec!(0)), dec!(0));
    }

    #[test]
    fn normalized_ratio_equal() {
        assert_eq!(normalized_ratio(dec!(5), dec!(5)), dec!(0));
    }

    #[test]
    fn normalized_ratio_negative() {
        assert_eq!(normalized_ratio(dec!(1), dec!(3)), dec!(-0.5));
    }

    // ── cosine_similarity ──

    #[test]
    fn cosine_identical_vectors() {
        let v = [dec!(1), dec!(1), dec!(1), dec!(1)];
        let sim = cosine_similarity(v, v);
        // Should be 1.0 (or very close)
        assert!((sim - Decimal::ONE).abs() < dec!(0.0001));
    }

    #[test]
    fn cosine_opposite_vectors() {
        let a = [dec!(1), dec!(1), dec!(1), dec!(1)];
        let b = [dec!(-1), dec!(-1), dec!(-1), dec!(-1)];
        let sim = cosine_similarity(a, b);
        assert!((sim + Decimal::ONE).abs() < dec!(0.0001));
    }

    #[test]
    fn cosine_zero_vector() {
        let a = [dec!(1), dec!(2), dec!(3), dec!(4)];
        let b = [dec!(0), dec!(0), dec!(0), dec!(0)];
        assert_eq!(cosine_similarity(a, b), dec!(0));
    }

    #[test]
    fn cosine_orthogonal() {
        let a = [dec!(1), dec!(0), dec!(0), dec!(0)];
        let b = [dec!(0), dec!(1), dec!(0), dec!(0)];
        let sim = cosine_similarity(a, b);
        assert!(sim.abs() < dec!(0.0001));
    }

    // ── jaccard ──

    #[test]
    fn jaccard_identical() {
        let a: HashSet<i32> = [1, 2, 3].into();
        let b: HashSet<i32> = [1, 2, 3].into();
        assert_eq!(jaccard(&a, &b), Decimal::ONE);
    }

    #[test]
    fn jaccard_disjoint() {
        let a: HashSet<i32> = [1, 2].into();
        let b: HashSet<i32> = [3, 4].into();
        assert_eq!(jaccard(&a, &b), Decimal::ZERO);
    }

    #[test]
    fn jaccard_partial() {
        let a: HashSet<i32> = [1, 2, 3].into();
        let b: HashSet<i32> = [2, 3, 4].into();
        // intersection = {2,3} = 2, union = {1,2,3,4} = 4 → 0.5
        assert_eq!(jaccard(&a, &b), dec!(0.5));
    }

    #[test]
    fn jaccard_empty() {
        let a: HashSet<i32> = HashSet::new();
        let b: HashSet<i32> = HashSet::new();
        assert_eq!(jaccard(&a, &b), Decimal::ZERO);
    }

    // ── decimal_sqrt ──

    #[test]
    fn sqrt_four() {
        let result = decimal_sqrt(dec!(4));
        assert!((result - dec!(2)).abs() < dec!(0.0001));
    }

    #[test]
    fn sqrt_zero() {
        assert_eq!(decimal_sqrt(dec!(0)), dec!(0));
    }
}

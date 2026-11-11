//! Axis scales for the charts (integer arithmetic on minor units).

/// "Nice" tick values covering `min..=max` (minor units), with a step of
/// 1, 2, 2.5 or 5 × 10ⁿ whole units, aiming at about `target` ticks. The
/// first tick is ≤ `min` and the last ≥ `max`.
pub fn nice_ticks(min: i64, max: i64, target: usize, unit: i64) -> Vec<i64> {
    let (min, max) = if min <= max { (min, max) } else { (max, min) };
    let target = target.max(2) as i64;
    let span = (max - min).max(unit);
    let raw = (span + target - 2) / (target - 1);
    // Smallest nice step ≥ raw, in minor units (step ≥ 1 whole unit).
    let mut magnitude = unit;
    while magnitude * 10 <= raw {
        magnitude *= 10;
    }
    let step = [
        magnitude,
        magnitude * 2,
        magnitude * 5 / 2,
        magnitude * 5,
        magnitude * 10,
    ]
    .into_iter()
    .find(|s| *s >= raw)
    .unwrap();
    let first = min.div_euclid(step) * step;
    let mut ticks = vec![first];
    while *ticks.last().unwrap() < max || ticks.len() < 2 {
        ticks.push(ticks.last().unwrap() + step);
    }
    ticks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_cover_the_range_with_round_steps() {
        let t = nice_ticks(390_000, 450_000, 3, 100);
        assert_eq!(t, vec![350_000, 400_000, 450_000]);
        let t = nice_ticks(1_400_000, 1_750_000, 4, 100);
        assert_eq!(t, vec![1_400_000, 1_600_000, 1_800_000]);
        let t = nice_ticks(0, 360_000, 4, 100);
        assert_eq!(t, vec![0, 200_000, 400_000]);
        assert_eq!(nice_ticks(5_000, 5_000, 3, 100), vec![5_000, 5_100]);
        assert_eq!(nice_ticks(-1_000, 1_000, 3, 100), vec![-1_000, 0, 1_000]);
    }
}

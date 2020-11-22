/// convert a cpu string (kubernetes like) into a float. It supports millis cpu
/// examples:
/// 250m = 0.25 cpu
/// 500m = 0.50 cpu
/// 1000m = 1 cpu
/// 1.25 = 1.25
pub fn cpu_string_to_float<T: Into<String>>(cpu: T) -> f32 {
    let cpu = cpu.into();
    if cpu.is_empty() {
        return 0.0;
    }

    if !cpu.ends_with("m") {
        // the value is not in millis
        return match cpu.parse::<f32>() {
            Ok(v) if v >= 0.0 => v,
            _ => 0.0,
        };
    }

    // the result is in millis, so convert it to float
    let cpu = cpu.replace("m", "");
    match cpu.parse::<f32>() {
        Ok(v) if v >= 0.0 => v / 1000.0,
        _ => 0.0,
    }
}

/// convert ki to mi
pub fn ki_to_mi<T: Into<String>>(ram: T) -> u32 {
    let ram = ram.into().to_lowercase().replace("ki", "");
    match ram.parse::<u32>() {
        Ok(v) => v / 1000,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use crate::unit_conversion::cpu_string_to_float;
    use crate::unit_conversion::ki_to_mi;

    #[test]
    fn test_cpu_conversions() {
        assert_eq!(cpu_string_to_float("250m"), 0.25);
        assert_eq!(cpu_string_to_float("500m"), 0.5);
        assert_eq!(cpu_string_to_float("1500m"), 1.5);
        assert_eq!(cpu_string_to_float("1.5"), 1.5);
        assert_eq!(cpu_string_to_float("0"), 0.0);
        assert_eq!(cpu_string_to_float("0m"), 0.0);
        assert_eq!(cpu_string_to_float("-250m"), 0.0);
        assert_eq!(cpu_string_to_float("-10"), 0.0);
        assert_eq!(cpu_string_to_float("1000"), 1000.0);
    }

    #[test]
    fn test_kib_to_mib_conversions() {
        assert_eq!(ki_to_mi("15564756Ki"), 15_564);
    }
}

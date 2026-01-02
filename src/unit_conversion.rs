use crate::errors::CommandError;

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

    if !cpu.ends_with('m') {
        // the value is not in millis
        return match cpu.parse::<f32>() {
            Ok(v) if v >= 0.0 => v,
            _ => 0.0,
        };
    }

    // the result is in millis, so convert it to float
    let cpu = cpu.replace('m', "");
    match cpu.parse::<f32>() {
        Ok(v) if v >= 0.0 => v / 1000.0,
        _ => 0.0,
    }
}

pub fn extract_volume_size(string_to_parse: String) -> Result<u32, CommandError> {
    let first_non_digit_index = match string_to_parse.find(|c: char| !c.is_numeric()) {
        None => string_to_parse.len(),
        Some(index) => index,
    };
    match string_to_parse[..first_non_digit_index].parse::<u32>() {
        Ok(value) => Ok(value),
        Err(e) => Err(CommandError::new_from_safe_message(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use crate::unit_conversion::cpu_string_to_float;
    use crate::unit_conversion::extract_volume_size;

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
    fn test_any_extract_volume_size() {
        assert_eq!(extract_volume_size("10Gi".to_string()).expect("unable to get volume size"), 10);
        assert_eq!(
            extract_volume_size("100Gi".to_string()).expect("unable to get volume size"),
            100
        );
        assert_eq!(
            extract_volume_size("1000Gi".to_string()).expect("unable to get volume size"),
            1000
        );
        assert_eq!(
            extract_volume_size("10000Gi".to_string()).expect("unable to get volume size"),
            10000
        );
        assert!(extract_volume_size("toto".to_string()).is_err())
    }
}

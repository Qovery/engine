use serde::{Deserialize, Serialize};
use std::fmt;

/// AWS EBS gp3 disk IOPS configuration
///
/// AWS allows IOPS values between 3,000 and 16,000 for gp3 volumes.
/// The IOPS to throughput ratio must not exceed 500:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DiskIops(u32);

impl DiskIops {
    /// Minimum allowed IOPS for AWS gp3 volumes
    pub const MIN: u32 = 3000;
    /// Maximum allowed IOPS for AWS gp3 volumes
    pub const MAX: u32 = 16000;

    /// Creates a new DiskIops value, validating it's within AWS constraints
    pub fn new(value: u32) -> Result<Self, String> {
        if !(Self::MIN..=Self::MAX).contains(&value) {
            return Err(format!("IOPS must be between {} and {}, got {}", Self::MIN, Self::MAX, value));
        }
        Ok(Self(value))
    }

    /// Returns the raw IOPS value
    pub fn value(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for DiskIops {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// AWS EBS gp3 disk throughput configuration in MB/s
///
/// AWS allows throughput values between 125 MB/s and 1,000 MB/s for gp3 volumes.
/// The IOPS to throughput ratio must not exceed 500:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DiskThroughput(u32);

impl DiskThroughput {
    /// Minimum allowed throughput for AWS gp3 volumes (MB/s)
    pub const MIN: u32 = 125;
    /// Maximum allowed throughput for AWS gp3 volumes (MB/s)
    pub const MAX: u32 = 1000;

    /// Creates a new DiskThroughput value, validating it's within AWS constraints
    pub fn new(value: u32) -> Result<Self, String> {
        if !(Self::MIN..=Self::MAX).contains(&value) {
            return Err(format!(
                "Throughput must be between {} and {} MB/s, got {}",
                Self::MIN,
                Self::MAX,
                value
            ));
        }
        Ok(Self(value))
    }

    /// Returns the raw throughput value in MB/s
    pub fn value(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for DiskThroughput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disk_iops_valid() {
        assert!(DiskIops::new(3000).is_ok());
        assert!(DiskIops::new(16000).is_ok());
        assert!(DiskIops::new(10000).is_ok());
    }

    #[test]
    fn test_disk_iops_invalid() {
        assert!(DiskIops::new(2999).is_err());
        assert!(DiskIops::new(16001).is_err());
        assert!(DiskIops::new(0).is_err());
    }

    #[test]
    fn test_disk_throughput_valid() {
        assert!(DiskThroughput::new(125).is_ok());
        assert!(DiskThroughput::new(1000).is_ok());
        assert!(DiskThroughput::new(500).is_ok());
    }

    #[test]
    fn test_disk_throughput_invalid() {
        assert!(DiskThroughput::new(124).is_err());
        assert!(DiskThroughput::new(1001).is_err());
        assert!(DiskThroughput::new(0).is_err());
    }

    #[test]
    fn test_serde_disk_iops() {
        let iops = DiskIops::new(5000).unwrap();
        let serialized = serde_json::to_string(&iops).unwrap();
        assert_eq!(serialized, "5000");

        let deserialized: DiskIops = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, iops);
    }

    #[test]
    fn test_serde_disk_throughput() {
        let throughput = DiskThroughput::new(250).unwrap();
        let serialized = serde_json::to_string(&throughput).unwrap();
        assert_eq!(serialized, "250");

        let deserialized: DiskThroughput = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, throughput);
    }
}

use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DiskSize {
    Gib(u32),
}

impl DiskSize {
    pub fn to_gib(&self) -> u32 {
        match self {
            DiskSize::Gib(size) => *size,
        }
    }

    /// Returns the disk size as a string in the format "{size}Gi"
    pub fn to_gib_string(&self) -> String {
        format!("{}Gi", self.to_gib())
    }
}

impl Default for DiskSize {
    fn default() -> Self {
        DiskSize::Gib(0) // or any default you want
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disk_size_to_gib() {
        let size = DiskSize::Gib(50);
        assert_eq!(size.to_gib(), 50);
    }

    #[test]
    fn test_disk_size_to_gib_string() {
        let size = DiskSize::Gib(100);
        assert_eq!(size.to_gib_string(), "100Gi");
    }

    #[test]
    fn test_disk_size_serialization() {
        let size = DiskSize::Gib(200);
        let serialized = serde_json::to_string(&size).unwrap();
        assert_eq!(serialized, "200");

        let deserialized: DiskSize = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, size);
    }
}

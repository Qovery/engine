use azure_storage::CloudLocation;
use std::ops::Deref;
use strum_macros::EnumIter;

#[derive(PartialEq, Eq, Debug, Clone, EnumIter, Hash)]
pub enum AzureStorageRegion {
    Public { account: String },
    China { account: String },
    Emulator { address: String, port: u16 },
    Custom { account: String, uri: String },
}

impl From<CloudLocation> for AzureStorageRegion {
    fn from(source: CloudLocation) -> Self {
        match source {
            CloudLocation::Public { account } => Self::Public { account },
            CloudLocation::China { account } => Self::China { account },
            CloudLocation::Emulator { address, port } => Self::Emulator { address, port },
            CloudLocation::Custom { account, uri } => Self::Custom { account, uri },
        }
    }
}

// This wrapper to allow to implement From<AzureRegion> for CloudLocation without being
// yelled at by the orphan rule
pub struct CloudLocationWrapper(CloudLocation);

impl From<AzureStorageRegion> for CloudLocationWrapper {
    fn from(value: AzureStorageRegion) -> CloudLocationWrapper {
        match value {
            AzureStorageRegion::Public { account } => CloudLocationWrapper(CloudLocation::Public { account }),
            AzureStorageRegion::China { account } => CloudLocationWrapper(CloudLocation::China { account }),
            AzureStorageRegion::Emulator { address, port } => {
                CloudLocationWrapper(CloudLocation::Emulator { address, port })
            }
            AzureStorageRegion::Custom { account, uri } => CloudLocationWrapper(CloudLocation::Custom { account, uri }),
        }
    }
}

impl Deref for CloudLocationWrapper {
    type Target = CloudLocation;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use azure_storage::CloudLocation;

    #[test]
    fn test_azure_storage_region_from_cloud_location() {
        // setup:
        struct TestCase {
            cloud_location: CloudLocation,
            expected: AzureStorageRegion,
        }

        let test_cases = vec![
            TestCase {
                cloud_location: CloudLocation::Public {
                    account: "account".to_string(),
                },
                expected: AzureStorageRegion::Public {
                    account: "account".to_string(),
                },
            },
            TestCase {
                cloud_location: CloudLocation::China {
                    account: "account".to_string(),
                },
                expected: AzureStorageRegion::China {
                    account: "account".to_string(),
                },
            },
            TestCase {
                cloud_location: CloudLocation::Emulator {
                    address: "address".to_string(),
                    port: 10000,
                },
                expected: AzureStorageRegion::Emulator {
                    address: "address".to_string(),
                    port: 10000,
                },
            },
            TestCase {
                cloud_location: CloudLocation::Custom {
                    account: "account".to_string(),
                    uri: "uri".to_string(),
                },
                expected: AzureStorageRegion::Custom {
                    account: "account".to_string(),
                    uri: "uri".to_string(),
                },
            },
        ];

        for tc in test_cases {
            // execute:
            let azure_storage_region = AzureStorageRegion::from(tc.cloud_location);

            // verify:
            assert_eq!(azure_storage_region, tc.expected);
        }
    }

    #[test]
    fn test_azure_storage_region_to_cloud_location() {
        // setup:
        struct TestCase {
            azure_storage_region: AzureStorageRegion,
            expected: CloudLocation,
        }

        let test_cases = vec![
            TestCase {
                azure_storage_region: AzureStorageRegion::Public {
                    account: "account".to_string(),
                },
                expected: CloudLocation::Public {
                    account: "account".to_string(),
                },
            },
            TestCase {
                azure_storage_region: AzureStorageRegion::China {
                    account: "account".to_string(),
                },
                expected: CloudLocation::China {
                    account: "account".to_string(),
                },
            },
            TestCase {
                azure_storage_region: AzureStorageRegion::Emulator {
                    address: "address".to_string(),
                    port: 10000,
                },
                expected: CloudLocation::Emulator {
                    address: "address".to_string(),
                    port: 10000,
                },
            },
            TestCase {
                azure_storage_region: AzureStorageRegion::Custom {
                    account: "account".to_string(),
                    uri: "uri".to_string(),
                },
                expected: CloudLocation::Custom {
                    account: "account".to_string(),
                    uri: "uri".to_string(),
                },
            },
        ];

        for tc in test_cases {
            // execute:
            let cloud_location: CloudLocation = CloudLocationWrapper::from(tc.azure_storage_region).to_owned();

            // verify:
            assert!(match (tc.expected, cloud_location) {
                (CloudLocation::Public { account: a_account }, CloudLocation::Public { account: b_account }) =>
                    a_account == b_account,
                (CloudLocation::China { account: a_account }, CloudLocation::China { account: b_account }) =>
                    a_account == b_account,
                (
                    CloudLocation::Emulator {
                        address: a_address,
                        port: a_port,
                    },
                    CloudLocation::Emulator {
                        address: b_address,
                        port: b_port,
                    },
                ) => a_address == b_address && a_port == b_port,
                (
                    CloudLocation::Custom {
                        account: a_account,
                        uri: a_uri,
                    },
                    CloudLocation::Custom {
                        account: b_account,
                        uri: b_uri,
                    },
                ) => a_account == b_account && a_uri == b_uri,
                _ => false,
            });
        }
    }
}

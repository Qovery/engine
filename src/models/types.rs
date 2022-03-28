// Those types are just marker types that are use to tag our struct/object model
pub struct AWS {}
pub struct DO {}
pub struct SCW {}

// CloudProvider trait allows to derive all the custom type we need per provider,
// with our marker type defined above to be able to select the correct one
pub trait CloudProvider {
    type AppExtraSettings;
    type DbExtraSettings;
    type RouterExtraSettings;
    type StorageTypes;

    fn short_name() -> &'static str;
    fn full_name() -> &'static str;
    fn registry_short_name() -> &'static str;
    fn registry_full_name() -> &'static str;
    fn helm_directory_name() -> &'static str;
}

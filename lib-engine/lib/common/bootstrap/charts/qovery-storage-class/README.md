# qovery-storage-class

Qovery StorageClasses for the Engine v2 platform catalog. This chart is installed only by the
`qovery-storage-class` component of the `qovery-self-managed-v0` template (BYOK v2 clusters); see
`platform-catalog/components/qovery-storage-class/config/README.md`. Every class it creates carries
the annotation `qovery.com/source: platform-catalog`.

The class definitions mirror the legacy `lib-engine/lib/{aws,gcp,scaleway}/bootstrap/charts/q-storageclass-*`
charts, which the legacy bootstrap still installs on Qovery-managed and legacy BYOK clusters. Change
both together: `storage_classes_match_the_legacy_provider_charts_without_a_default_class` in
`tools/platform-catalog-tests` fails when they differ. StorageClass parameters are immutable, so a
parameter change needs a new class name.

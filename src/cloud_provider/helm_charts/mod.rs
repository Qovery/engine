use crate::cloud_provider::helm::CommonChart;

pub mod qovery_storage_class_chart;

pub trait ToCommonHelmChart {
    fn to_common_helm_chart(&self) -> CommonChart;
}

use uuid::Uuid;

pub struct FeatureRepository;

impl FeatureRepository {
    pub(crate) fn check_if_image_already_exist_in_the_registry_of_the_cluster(cluster_id: &Uuid) -> bool {
        cluster_id.to_string() == "be9e22b0-d05a-4330-b5b5-547667d380fd"    // Qovery tests AWS
       || cluster_id.to_string() == "4229c4d9-d216-4271-a5f3-4010135c1e98" // RxVantage
    }
}

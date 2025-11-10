use std::fs;

// Hack
// Binary gke-gcloud-auth-plugin used to access kube (in kubeconfig) write cache file in ~/.kube/gke_gcloud_auth_plugin_cache
// it is not possible to modify the path of this cache to be user/deployment specific
// https://github.com/kubernetes/cloud-provider-gcp/issues/554
// So we remove it before running gcloud auth activate-service-account **but** it does break the isolation per deployment
// meaning the engine will never be able to do concurrent deployment on different gcp cluster at the same time
// It is not an issue for now as 1 engine is running at max 1 deployment
pub fn remove_gke_gcloud_auth_plugin_cache() {
    let path = std::env::home_dir()
        .unwrap_or_default()
        .join(".kube")
        .join("gke_gcloud_auth_plugin_cache");
    info!("Removing gke_gcloud_auth_plugin_cache file {}", path.display());
    let _ = fs::remove_file(path);
}

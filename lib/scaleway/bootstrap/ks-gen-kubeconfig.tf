resource "local_file" "kubeconfig" {
  filename = "test-cluster/test-cluster.yaml" # TODO(benjaminch): use "{{ s3_kubeconfig_bucket }}/${var.kubernetes_cluster_id}.yaml"
  content = scaleway_k8s_cluster.kubernetes_cluster.kubeconfig[0].config_file
  file_permission = "0644"
  depends_on = [scaleway_k8s_cluster.kubernetes_cluster] # TODO(benjaminch): use `scw_ks_worker_node in scw_ks_worker_nodes` to get all nodes names
}

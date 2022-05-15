resource "local_file" "kubeconfig" {
  filename = "{{ object_storage_kubeconfig_bucket }}/${var.kubernetes_cluster_id}.yaml"
  content = scaleway_k8s_cluster.kubernetes_cluster.kubeconfig[0].config_file
  file_permission = "0644"
  depends_on = [scaleway_k8s_cluster.kubernetes_cluster]
}

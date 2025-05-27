output "loki_storage_config_scaleway_s3" {
    value = "s3://${urlencode(var.scaleway_access_key)}:${urlencode(var.scaleway_secret_key)}@s3.${var.region}.scw.cloud/{{ object_storage_logs_bucket }}"
    sensitive = true
}
output "kubeconfig" {
  value =  scaleway_k8s_cluster.kubernetes_cluster.kubeconfig[0].config_file
  sensitive = true
}

output "cluster_name" {
  value = scaleway_k8s_cluster.kubernetes_cluster.name
}

output "cluster_id" {
  value = scaleway_k8s_cluster.kubernetes_cluster.id
}

output "private_network_id" {
  value = scaleway_vpc_private_network.private_network.id
}

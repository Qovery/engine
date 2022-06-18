locals {
  kubeconfig_base64 = base64encode(scaleway_k8s_cluster.kubernetes_cluster.kubeconfig[0].config_file)
}
// do not run for tests clusters to avoid uncleaned info.
// do not try to use count into resource, it will fails trying to connect to vault
{% if vault_auth_method != "none" and not test_cluster %}
resource "vault_generic_secret" "cluster-access" {
  path = "official-clusters-access/${var.kubernetes_full_cluster_id}"

  data_json = <<EOT
{
  "cloud_provider": "${var.cloud_provider}",
  "cluster_name": "${var.kubernetes_cluster_name}",
  "KUBECONFIG_b64": "${local.kubeconfig_base64}",
  "organization_id": "${var.organization_id}",
  "test_cluster": "${var.test_cluster}",
  "grafana_login": "{{ grafana_admin_user }}",
  "grafana_password": "{{ grafana_admin_password }}",
  "SCW_DEFAULT_PROJECT_ID": "{{ scaleway_project_id }}",
  "SCW_ACCESS_KEY": "{{ scaleway_access_key }}",
  "SCW_SECRET_KEY": "{{ scaleway_secret_key }}",
  "SCW_DEFAULT_REGION": "{{ scw_region }}",
  "SCW_DEFAULT_ZONE": "{{ scw_zone }}"
}
EOT

  depends_on = [
    scaleway_k8s_cluster.kubernetes_cluster,
  ]
}
{% endif %}
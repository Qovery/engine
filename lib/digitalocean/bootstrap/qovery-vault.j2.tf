locals {
  kubeconfig_base64 = base64encode(digitalocean_kubernetes_cluster.kubernetes_cluster.kube_config.0.raw_config)
}
// do not run for tests clusters to avoid uncleaned info.
// do not try to use count into resource, it will fails trying to connect to vault
{% if vault_auth_method != "none" and not test_cluster %}
resource "vault_generic_secret" "cluster-access" {
  path = "official-clusters-access/${var.organization_id}-${var.kubernetes_cluster_id}"

  data_json = <<EOT
{
  "cloud_provider": "${var.cloud_provider}",
  "cluster_name": "${var.kubernetes_cluster_name}",
  "KUBECONFIG_b64": "${local.kubeconfig_base64}",
  "organization_id": "${var.organization_id}",
  "test_cluster": "${var.test_cluster}",
  "grafana_login": "{{ grafana_admin_user }}",
  "grafana_password": "{{ grafana_admin_password }}",
  "DIGITAL_OCEAN_DEFAULT_REGION": "${var.region}",
  "DIGITAL_OCEAN_SPACES_ACCESS_ID": "${var.space_access_id}",
  "DIGITAL_OCEAN_SPACES_SECRET_ID": "${var.space_secret_key}",
  "DIGITAL_OCEAN_TOKEN": "{{ digitalocean_token }}",
}
EOT

  depends_on = [
    digitalocean_kubernetes_cluster.kubernetes_cluster,
]
}
{% endif %}
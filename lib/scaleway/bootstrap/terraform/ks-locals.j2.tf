locals {
  tags_ks = {
    ClusterId          = var.kubernetes_cluster_id
    ClusterLongId      = var.kubernetes_cluster_long_id
    OrganizationId     = var.organization_id,
    OrganizationLongId = var.organization_long_id,
    Region             = var.region
    creationDate       = time_static.on_cluster_create.rfc3339
    QoveryProduct      = "Kapsule"
    {% if resource_expiration_in_seconds > -1 %}ttl = var.resource_expiration_in_seconds{% endif %}
  }
  tags_ks_list = [for i, v in local.tags_ks : "${i}=${v}"] # NOTE: Scaleway doesn't support KV style tags
}

resource "time_static" "on_cluster_create" {}

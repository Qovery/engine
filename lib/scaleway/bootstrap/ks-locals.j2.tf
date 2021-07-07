locals {
  tags_ks = {
    ClusterId      = var.kubernetes_cluster_id
    ClusterName    = var.kubernetes_cluster_name
    Region         = var.region
    creationDate   = time_static.on_cluster_create.rfc3339
    {% if resource_expiration_in_seconds is defined %}ttl = var.resource_expiration_in_seconds{% endif %}
  }
  tags_ks_list = [for i, v in local.tags_ks : "${i}=${v}"] # NOTE: Scaleway doesn't support KV style tags
}

resource "time_static" "on_cluster_create" {}

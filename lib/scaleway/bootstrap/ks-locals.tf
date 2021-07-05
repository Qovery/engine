locals {
  tags_ks = {
    ClusterId      = "test-cluster-123", # TODO(benjaminch) : use : var.kubernetes_cluster_id
    ClusterName    = "test-cluster", # TODO(benjaminch) : use : var.kubernetes_cluster_name,
    Region         = "fr-par", # TODO(benjaminch) : use : var.region
    creationDate   = time_static.on_cluster_create.rfc3339,
    # TODO(benjaminch): un-comment
    # {% if resource_expiration_in_seconds is defined %}ttl = var.resource_expiration_in_seconds{% endif %}
  }
  tags_ks_list = [for i, v in local.tags_ks : "${i}=${v}"] # NOTE: Scaleway doesn't support KV style tags
}

resource "time_static" "on_cluster_create" {}

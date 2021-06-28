locals {
  tags_ks = [
    "test-cluster-123", # TODO(benjaminch) : use : var.kubernetes_cluster_id
    "test-cluster", # TODO(benjaminch) : use : var.kubernetes_cluster_name,
    "fr-par", # TODO(benjaminch) : use : var.region
    time_static.on_cluster_create.rfc3339,
    # TODO(benjaminch): un-comment
    # {% if resource_expiration_in_seconds is defined %}var.resource_expiration_in_seconds{% endif %}
  ]
}

resource "time_static" "on_cluster_create" {}

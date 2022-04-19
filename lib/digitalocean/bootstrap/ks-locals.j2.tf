locals {
  tags_ks = {
    ClusterId      = var.kubernetes_full_cluster_id
    OrganizationId = var.organization_id,
    Region         = var.region
    creationDate   = time_static.on_cluster_create.rfc3339
    {% if resource_expiration_in_seconds is defined %}ttl = var.resource_expiration_in_seconds{% endif %}
  }
  # NOTE:
  # - Digital Ocean doesn't support KV style tags
  # - Digital Ocean tags may contain lowercase letters, numbers, colons, dashes, and underscores; there is a limit of 255 characters per tag
  tags_ks_list = [for i, v in local.tags_ks : "${i}:${v}"]
}

resource "time_static" "on_cluster_create" {}

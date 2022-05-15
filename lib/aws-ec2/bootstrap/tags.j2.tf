locals {
  tags_common = {
    ClusterId = var.kubernetes_cluster_id
    ClusterLongId = var.kubernetes_full_cluster_id
    OrganizationId = var.organization_id,
    Region = var.region
    creationDate = time_static.on_ec2_create.rfc3339
    {% if resource_expiration_in_seconds is defined %}ttl = var.resource_expiration_in_seconds{% endif %}
  }

  tags_ec2 = merge(
    local.tags_common,
    {
      "Service" = "EC2"
    }
  )
}
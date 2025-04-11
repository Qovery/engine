locals {
  # A set of tags that are common to all resources
  tags_common = {
      cluster_id                                      = "{{ kubernetes_cluster_id }}",
      cluster_long_id                                 = "{{ kubernetes_cluster_long_id }}",
      organization_id                                 = "{{ organization_id }}",
      organization_long_id                            = "{{ organization_long_id }}",
      region                                          = "{{ azure_location }}",
      creation_date                                   = time_static.on_cluster_create.unix,
      qovery_product                                  = "aks",
      {% if resource_expiration_in_seconds > -1 %}ttl = {{ resource_expiration_in_seconds }} {% endif %}
  }
}
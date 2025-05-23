resource "azurerm_kubernetes_cluster" "primary" {
  name                = var.kubernetes_cluster_name
  location            = local.location
  resource_group_name = azurerm_resource_group.main.name
  dns_prefix          = var.kubernetes_cluster_name

  oidc_issuer_enabled       = true
  workload_identity_enabled = true

  kubernetes_version = var.kubernetes_version

  default_node_pool {
    name    = "{{ node_group_default.name }}"
    vm_size = "{{ node_group_default.instance_type }}"
    # only_critical_addons_enabled = true # tainting the nodes with CriticalAddonsOnly=true:NoSchedule to avoid scheduling workloads on the system node pool
    zones                  = ["{{ node_group_default.zone }}"]
    {% if node_group_default.zone == "1" %}
    vnet_subnet_id         = azurerm_subnet.node_cidr_zone_1.id
    {% elif node_group_default.zone == "2" %}
    vnet_subnet_id         = azurerm_subnet.node_cidr_zone_2.id
    {% elif node_group_default.zone == "3" %}
    vnet_subnet_id         = azurerm_subnet.node_cidr_zone_3.id
    {% else %}
    # this is an error and should fail
    vnet_subnet_id         = "unsupported"
    {% endif %}
    auto_scaling_enabled   = true
    min_count              = {{ node_group_default.min_nodes }}
    max_count              = {{ node_group_default.max_nodes }}
    node_public_ip_enabled = false
    orchestrator_version   = var.kubernetes_version # Keep nodes up to date with control plane
    temporary_name_for_rotation = "{{ node_group_default.name }}temp"
    upgrade_settings {
      drain_timeout_in_minutes      = 0
      max_surge                     = "10%"
      node_soak_duration_in_minutes = 0
    }
  }

  identity {
    type = "SystemAssigned"
  }

  network_profile {
    network_plugin      = "azure"
    network_plugin_mode = "overlay"
    network_policy      = "cilium"
    network_data_plane  = "cilium"
    load_balancer_sku   = "standard"
    outbound_type       = "userAssignedNATGateway"
    pod_cidr            = local.pod_cidr
    service_cidr        = local.service_cidr
    dns_service_ip      = local.dns_service_ip
  }

  # TODO(benjaminch): Azure integration, to be variabilized
  maintenance_window {
    allowed {
      day   = "Tuesday"
      hours = [21, 22, 23]
    }
  }

  tags = local.tags_aks

  depends_on = [
    {% for zone in azure_zones %}
    azurerm_subnet_nat_gateway_association.zone_{{ zone }},
    {% endfor %}
  ]
}

{% for node_group in node_groups_additional %}
resource "azurerm_kubernetes_cluster_node_pool" "node_pool_zone_{{ node_group.zone }}" {
  name    = "{{ node_group.name }}"
  kubernetes_cluster_id  = azurerm_kubernetes_cluster.primary.id
  vm_size = "{{ node_group.instance_type }}"
  # only_critical_addons_enabled = true # tainting the nodes with CriticalAddonsOnly=true:NoSchedule to avoid scheduling workloads on the system node pool
  zones                  = ["{{ node_group.zone }}"]
  {% if node_group.zone == "1" %}
  vnet_subnet_id         = azurerm_subnet.node_cidr_zone_1.id
  {% elif node_group.zone == "2" %}
  vnet_subnet_id         = azurerm_subnet.node_cidr_zone_2.id
  {% elif node_group.zone == "3" %}
  vnet_subnet_id         = azurerm_subnet.node_cidr_zone_3.id
  {% else %}
  # this is an error and should fail
  vnet_subnet_id         = "unsupported"
  {% endif %}
  auto_scaling_enabled   = true
  min_count              = {{ node_group.min_nodes }}
  max_count              = {{ node_group.max_nodes }}
  node_public_ip_enabled = false
  orchestrator_version   = var.kubernetes_version # Keep nodes up to date with control plane
  temporary_name_for_rotation = "{{ node_group.name }}temp"

  tags = local.tags_aks
}
{% endfor %}

# Update the AKS cluster to enable NAP using the azapi provider
# resource "azapi_update_resource" "nap" {
#   type                    = "Microsoft.ContainerService/managedClusters@2024-09-02-preview"
#   resource_id             = azurerm_kubernetes_cluster.primary.id
#   ignore_missing_property = true
#   body = {
#     properties = {
#       nodeProvisioningProfile = {
#         mode = "Auto"
#       }
#     }
#   }
# }

# data "azurerm_public_ip" "example" {
#   name                = reverse(split("/", tolist(azurerm_kubernetes_cluster.primary.network_profile.0.load_balancer_profile.0.effective_outbound_ips)[0]))[0]
#   resource_group_name = azurerm_kubernetes_cluster.primary.node_resource_group
# }

# IAM
# AKS has 2 sp, one for the cluster and one for the nodes (kubelet)
resource "azurerm_role_assignment" "sp_contributor" { # for nginx ingress
  scope                = azurerm_resource_group.main.id
  principal_id         = azurerm_kubernetes_cluster.primary.identity.0.principal_id
  role_definition_name = "Contributor"
}

resource "time_static" "on_cluster_create" {}

locals {
  tags_aks = merge(
    local.tags_common,
    {
      "service" = "aks"
    }
  )
}

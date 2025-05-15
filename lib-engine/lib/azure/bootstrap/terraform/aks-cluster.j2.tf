resource "azurerm_kubernetes_cluster" "primary" {
  name                = var.kubernetes_cluster_name
  location            = local.location
  resource_group_name = azurerm_resource_group.main.name
  dns_prefix          = var.kubernetes_cluster_name

  oidc_issuer_enabled       = true
  workload_identity_enabled = true

  kubernetes_version = var.kubernetes_version

  default_node_pool {
    name    = "default1"
    vm_size = "Standard_DS2_v2" # TODO(benjaminch): hardcoded for now, to be variabilized if needed later one
    # only_critical_addons_enabled = true # tainting the nodes with CriticalAddonsOnly=true:NoSchedule to avoid scheduling workloads on the system node pool
    zones                  = ["1"]
    vnet_subnet_id         = azurerm_subnet.node_cidr_zone_1.id
    auto_scaling_enabled   = true
    min_count              = 1
    max_count              = 3
    node_public_ip_enabled = false
    orchestrator_version   = var.kubernetes_version # Keep nodes up to date with control plane
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

  maintenance_window {
    allowed {
      day   = "Saturday"
      hours = [21, 22, 23]
    }
  }

  tags = local.tags_aks

  depends_on = [
    azurerm_subnet_nat_gateway_association.zone_1,
    azurerm_subnet_nat_gateway_association.zone_2,
    azurerm_subnet_nat_gateway_association.zone_3
  ]
}

resource "azurerm_kubernetes_cluster_node_pool" "node_pool_zone_2" {
  name                   = "default2"
  kubernetes_cluster_id  = azurerm_kubernetes_cluster.primary.id
  vm_size                = "Standard_DS2_v2"
  zones                  = ["2"]
  vnet_subnet_id         = azurerm_subnet.node_cidr_zone_2.id
  auto_scaling_enabled   = true
  min_count              = 1
  max_count              = 3
  node_public_ip_enabled = false
  orchestrator_version   = var.kubernetes_version # Keep nodes up to date with control plane

  tags = local.tags_aks
}

resource "azurerm_kubernetes_cluster_node_pool" "node_pool_zone_3" {
  name                   = "default3"
  kubernetes_cluster_id  = azurerm_kubernetes_cluster.primary.id
  vm_size                = "Standard_DS2_v2"
  zones                  = ["3"]
  vnet_subnet_id         = azurerm_subnet.node_cidr_zone_3.id
  auto_scaling_enabled   = true
  min_count              = 1
  max_count              = 3
  node_public_ip_enabled = false
  orchestrator_version   = var.kubernetes_version # Keep nodes up to date with control plane

  tags = local.tags_aks
}


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

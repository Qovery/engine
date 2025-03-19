# karpenter
resource "azurerm_user_assigned_identity" "karpenter_msi" {
  location            = local.location
  name                = "karpentermsi"
  resource_group_name = azurerm_resource_group.main.name
}

resource "azurerm_federated_identity_credential" "karpenter_fid" {
  name                = "KARPENTER_FID"
  resource_group_name = azurerm_resource_group.main.name
  audience            = ["api://AzureADTokenExchange"]
  issuer              = azurerm_kubernetes_cluster.primary.oidc_issuer_url
  parent_id           = azurerm_user_assigned_identity.karpenter_msi.id
  subject             = "system:serviceaccount:kube-system:karpenter-sa"
}

resource "azurerm_role_assignment" "karpenter_rg_mc_virtual_machine_contributor" {
  scope                = azurerm_kubernetes_cluster.primary.node_resource_group_id
  principal_id         = azurerm_user_assigned_identity.karpenter_msi.principal_id
  role_definition_name = "Virtual Machine Contributor"
}

resource "azurerm_role_assignment" "karpenter_rg_mc_network_contributor" {
  scope                = azurerm_kubernetes_cluster.primary.node_resource_group_id
  principal_id         = azurerm_user_assigned_identity.karpenter_msi.principal_id
  role_definition_name = "Network Contributor"
}

resource "azurerm_role_assignment" "karpenter_rg_mc_managed_identity_operator" {
  scope                = azurerm_kubernetes_cluster.primary.node_resource_group_id
  principal_id         = azurerm_user_assigned_identity.karpenter_msi.principal_id
  role_definition_name = "Managed Identity Operator"
}

resource "azurerm_role_assignment" "karpenter_rg_virtual_machine_contributor" {
  scope                = azurerm_resource_group.main.id
  principal_id         = azurerm_user_assigned_identity.karpenter_msi.principal_id
  role_definition_name = "Virtual Machine Contributor"
}

resource "azurerm_role_assignment" "karpenter_rg_network_contributor" {
  scope                = azurerm_resource_group.main.id
  principal_id         = azurerm_user_assigned_identity.karpenter_msi.principal_id
  role_definition_name = "Network Contributor"
}

resource "azurerm_role_assignment" "karpenter_rg_managed_identity_operator" {
  scope                = azurerm_resource_group.main.id
  principal_id         = azurerm_user_assigned_identity.karpenter_msi.principal_id
  role_definition_name = "Managed Identity Operator"
}


# Loki
resource "azurerm_user_assigned_identity" "loki_msi" {
  location            = local.location
  name                = "lokimsi"
  resource_group_name = azurerm_resource_group.main.name
}

resource "azurerm_federated_identity_credential" "loki_fid" {
  name                = "LOKI_FID"
  resource_group_name = azurerm_resource_group.main.name
  audience            = ["api://AzureADTokenExchange"]
  issuer              = azurerm_kubernetes_cluster.primary.oidc_issuer_url
  parent_id           = azurerm_user_assigned_identity.loki_msi.id
  subject             = "system:serviceaccount:loki:loki"
}

resource "azurerm_role_assignment" "loki_msi_storage_blob_data_contributor" {
  scope                = azurerm_storage_account.loki_storage.id
  principal_id         = azurerm_user_assigned_identity.loki_msi.principal_id
  role_definition_name = "Storage Blob Data Contributor"
}

# karpenter
resource "azurerm_user_assigned_identity" "karpenter_msi" {
  location            = local.location
  name                = "karpentermsi"
  resource_group_name = azurerm_resource_group.main.name

  tags = local.tags_iam
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


# Storage
resource "azurerm_user_assigned_identity" "storage_msi" {
  location            = local.location
  name                = "qoverystoragemsi"
  resource_group_name = azurerm_resource_group.main.name

  tags = local.tags_iam
}

resource "azurerm_federated_identity_credential" "storage_fid" {
  name                = "STORAGE_FID"
  resource_group_name = azurerm_resource_group.main.name
  audience            = ["api://AzureADTokenExchange"]
  issuer              = azurerm_kubernetes_cluster.primary.oidc_issuer_url
  parent_id           = azurerm_user_assigned_identity.storage_msi.id
  subject             = "system:serviceaccount:qovery:qovery-storage"
}

resource "azurerm_role_assignment" "storage_msi_blob_data_contributor" {
  scope                = azurerm_storage_account.main_storage.id
  principal_id         = azurerm_user_assigned_identity.storage_msi.principal_id
  role_definition_name = "Storage Blob Data Contributor"
}

locals {
  tags_iam = merge(
    local.tags_common,
    {
      "service" = "aks"
    }
  )
}
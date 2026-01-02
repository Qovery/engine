{% if prometheus_enabled %}
resource "azurerm_user_assigned_identity" "thanos_msi" {
  location            = local.location
  name                = "thanosmsi"
  resource_group_name = azurerm_resource_group.main.name
  tags                = local.tags_iam
}

resource "azurerm_federated_identity_credential" "thanos_fid" {
  name                = "THANOS_FID"
  resource_group_name = azurerm_resource_group.main.name
  audience            = ["api://AzureADTokenExchange"]
  issuer              = azurerm_kubernetes_cluster.primary.oidc_issuer_url
  parent_id           = azurerm_user_assigned_identity.thanos_msi.id
subject             = "system:serviceaccount:qovery:kube-prometheus-stack-prometheus"
}

resource "azurerm_role_assignment" "thanos_msi_blob_data_contributor" {
  scope                = azurerm_storage_account.main_storage.id
  principal_id         = azurerm_user_assigned_identity.thanos_msi.principal_id
  role_definition_name = "Storage Blob Data Contributor"
}

resource "azurerm_federated_identity_credential" "thanos_query_fid" {
  name                = "THANOS_QUERY_FID"
  resource_group_name = azurerm_resource_group.main.name
  audience            = ["api://AzureADTokenExchange"]
  issuer              = azurerm_kubernetes_cluster.primary.oidc_issuer_url
  parent_id           = azurerm_user_assigned_identity.thanos_msi.id
  subject             = "system:serviceaccount:qovery:thanos-query"
}

resource "azurerm_federated_identity_credential" "thanos_storegateway_fid" {
  name                = "THANOS_STOREGATEWAY_FID"
  resource_group_name = azurerm_resource_group.main.name
  audience            = ["api://AzureADTokenExchange"]
  issuer              = azurerm_kubernetes_cluster.primary.oidc_issuer_url
  parent_id           = azurerm_user_assigned_identity.thanos_msi.id
  subject             = "system:serviceaccount:qovery:thanos-storegateway"
}

resource "azurerm_federated_identity_credential" "thanos_compactor_fid" {
  name                = "THANOS_COMPACTOR_FID"
  resource_group_name = azurerm_resource_group.main.name
  audience            = ["api://AzureADTokenExchange"]
  issuer              = azurerm_kubernetes_cluster.primary.oidc_issuer_url
  parent_id           = azurerm_user_assigned_identity.thanos_msi.id
  subject             = "system:serviceaccount:qovery:thanos-compactor"
}

resource "azurerm_federated_identity_credential" "thanos_bucketweb_fid" {
  name                = "THANOS_BUCKETWEB_FID"
  resource_group_name = azurerm_resource_group.main.name
  audience            = ["api://AzureADTokenExchange"]
  issuer              = azurerm_kubernetes_cluster.primary.oidc_issuer_url
  parent_id           = azurerm_user_assigned_identity.thanos_msi.id
  subject             = "system:serviceaccount:qovery:thanos-bucketweb"
}
{% endif %}

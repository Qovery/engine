output "kubeconfig" {
    sensitive = true
    depends_on = [azurerm_kubernetes_cluster.primary]
    value = azurerm_kubernetes_cluster.primary.kube_config_raw
}
output "aks_cluster_public_hostname" {
    sensitive = true
    depends_on = [azurerm_kubernetes_cluster.primary]
    value = azurerm_kubernetes_cluster.primary.kube_config[0].host
}
output "main_storage_account_name" { value = azurerm_storage_account.main_storage.name  }
output "main_storage_account_primary_access_key" {
    sensitive = true
    value = azurerm_storage_account.main_storage.primary_access_key
}
output "loki_logging_service_msi_client_id" { value = azurerm_user_assigned_identity.storage_msi.client_id  }

resource "azurerm_storage_account" "main_storage" {
  name                     = var.main_storage_account_name
  resource_group_name      = azurerm_resource_group.main.name
  location                 = local.location
  account_tier             = "Standard"
  account_replication_type = "ZRS"

  tags = local.tags_main
}

locals {
  tags_main = merge(
    local.tags_common,
    {
      "service" = "aks"
    }
  )
}
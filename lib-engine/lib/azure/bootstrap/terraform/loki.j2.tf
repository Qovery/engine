resource "azurerm_storage_account" "loki_storage" {
  name                     = "qoverylokistorage"
  resource_group_name      = azurerm_resource_group.main.name
  location                 = local.location
  account_tier             = "Standard"
  account_replication_type = "ZRS"
}

resource "azurerm_storage_container" "chunk_bucket" {
  name                  = "chunk"
  storage_account_id    = azurerm_storage_account.loki_storage.id
  container_access_type = "private"
}

resource "azurerm_storage_container" "ruler_bucket" {
  name                  = "ruler"
  storage_account_id    = azurerm_storage_account.loki_storage.id
  container_access_type = "private"
}

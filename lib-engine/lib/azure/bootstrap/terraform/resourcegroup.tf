 resource "azurerm_resource_group" "main" {
  name     = var.resource_group_name
  location = local.location

  tags = local.tags_resource_group
}

locals {
  tags_resource_group = merge(
    local.tags_common,
    {
      "service" = "aks"
    }
  )
}
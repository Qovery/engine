# Vnet
resource "azurerm_virtual_network" "vnet" {
  name                = "${var.kubernetes_cluster_name}-vnet"
  location            = local.location
  resource_group_name = azurerm_resource_group.main.name
  address_space       = [local.vnet_cidr]

  tags = local.tags_network
}

{% if "1" in azure_zones %}
resource "azurerm_subnet" "node_cidr_zone_1" {
  name                            = "${var.kubernetes_cluster_name}-node-subnet-zone-1"
  resource_group_name             = azurerm_resource_group.main.name
  address_prefixes                = [local.node_cidr_zone_1]
  virtual_network_name            = azurerm_virtual_network.vnet.name
  default_outbound_access_enabled = false # no default outbound access for internet
  service_endpoints               = ["Microsoft.Sql"]
}

# NAT Gateway Zone 1
resource "azurerm_public_ip" "nat_zone_1" {
  name                = "${var.kubernetes_cluster_name}-nat-ip-zone-1"
  location            = var.location
  resource_group_name = azurerm_resource_group.main.name
  allocation_method   = "Static"
  sku                 = "Standard"
  zones               = ["1"]

  tags = local.tags_network
}

resource "azurerm_nat_gateway" "zone_1" {
  name                = "${var.kubernetes_cluster_name}-nat-gateway-zone-1"
  location            = var.location
  resource_group_name = azurerm_resource_group.main.name
  sku_name            = "Standard"
  zones               = ["1"]

  tags = local.tags_network
}

resource "azurerm_nat_gateway_public_ip_association" "zone_1" {
  nat_gateway_id       = azurerm_nat_gateway.zone_1.id
  public_ip_address_id = azurerm_public_ip.nat_zone_1.id
}

resource "azurerm_subnet_nat_gateway_association" "zone_1" {
  subnet_id      = azurerm_subnet.node_cidr_zone_1.id
  nat_gateway_id = azurerm_nat_gateway.zone_1.id
}
{% endif %}

{% if "2" in azure_zones %}
resource "azurerm_subnet" "node_cidr_zone_2" {
  name                            = "${var.kubernetes_cluster_name}-node-subnet-zone-2"
  resource_group_name             = azurerm_resource_group.main.name
  address_prefixes                = [local.node_cidr_zone_2]
  virtual_network_name            = azurerm_virtual_network.vnet.name
  default_outbound_access_enabled = false # no default outbound access for internet
  service_endpoints               = ["Microsoft.Sql"]
}

# NAT Gateway Zone 2
resource "azurerm_public_ip" "nat_zone_2" {
  name                = "${var.kubernetes_cluster_name}-nat-ip-zone-2"
  location            = var.location
  resource_group_name = azurerm_resource_group.main.name
  allocation_method   = "Static"
  sku                 = "Standard"
  zones               = ["2"]

  tags = local.tags_network
}

resource "azurerm_nat_gateway" "zone_2" {
  name                = "${var.kubernetes_cluster_name}-nat-gateway-zone-2"
  location            = var.location
  resource_group_name = azurerm_resource_group.main.name
  sku_name            = "Standard"
  zones               = ["2"]

  tags = local.tags_network
}

resource "azurerm_nat_gateway_public_ip_association" "zone_2" {
  nat_gateway_id       = azurerm_nat_gateway.zone_2.id
  public_ip_address_id = azurerm_public_ip.nat_zone_2.id
}

resource "azurerm_subnet_nat_gateway_association" "zone_2" {
  subnet_id      = azurerm_subnet.node_cidr_zone_2.id
  nat_gateway_id = azurerm_nat_gateway.zone_2.id
}
{% endif %}

{% if "3" in azure_zones %}
resource "azurerm_subnet" "node_cidr_zone_3" {
  name                            = "${var.kubernetes_cluster_name}-node-subnet-zone-3"
  resource_group_name             = azurerm_resource_group.main.name
  address_prefixes                = [local.node_cidr_zone_3]
  virtual_network_name            = azurerm_virtual_network.vnet.name
  default_outbound_access_enabled = false # no default outbound access for internet
  service_endpoints               = ["Microsoft.Sql"]
}

# NAT Gateway Zone 3
resource "azurerm_public_ip" "nat_zone_3" {
  name                = "${var.kubernetes_cluster_name}-nat-ip-zone-3"
  location            = var.location
  resource_group_name = azurerm_resource_group.main.name
  allocation_method   = "Static"
  sku                 = "Standard"
  zones               = ["3"]

  tags = local.tags_network
}

resource "azurerm_nat_gateway" "zone_3" {
  name                = "${var.kubernetes_cluster_name}-nat-gateway-zone-3"
  location            = var.location
  resource_group_name = azurerm_resource_group.main.name
  sku_name            = "Standard"
  zones               = ["3"]

  tags = local.tags_network
}

resource "azurerm_nat_gateway_public_ip_association" "zone_3" {
  nat_gateway_id       = azurerm_nat_gateway.zone_3.id
  public_ip_address_id = azurerm_public_ip.nat_zone_3.id
}

resource "azurerm_subnet_nat_gateway_association" "zone_3" {
  subnet_id      = azurerm_subnet.node_cidr_zone_3.id
  nat_gateway_id = azurerm_nat_gateway.zone_3.id
}
{% endif %}

locals {
  tags_network = merge(
    local.tags_common,
    {
      "service" = "aks"
    }
  )
}
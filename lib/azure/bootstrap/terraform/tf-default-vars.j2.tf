variable "kubernetes_cluster_name" {
  description = "The name of the AKS cluster"
  type        = string
  default     = "{{ kubernetes_cluster_name }}"
}

variable "resource_group_name" {
  description = "The name of the resource group"
  type        = string
  default     = "{{ azure_resource_group_name }}"
}

variable "location" {
  description = "The Azure region where resources will be created"
  type        = string
  default     = "{{ azure_location }}"
}

variable "kubernetes_version" {
  description = "The version of Kubernetes to use for the AKS cluster"
  type        = string
  default     = "{{ kubernetes_cluster_version }}"
}

# variable "subnet_id" {
#   description = "The ID of the subnet where the AKS cluster will be deployed"
#   type        = string
# }

variable "enable_secrets_encryption" {
  description = "Enable Azure Key Vault Provider for Secrets Store CSI Driver"
  type        = bool
  default     = false
}

variable "enable_monitoring" {
  description = "Enable Azure Monitor for containers"
  type        = bool
  default     = true
}

variable "log_analytics_workspace_id" {
  description = "The ID of the Log Analytics workspace for container monitoring"
  type        = string
  default     = ""
}

variable "timeouts" {
  description = "Timeouts for resource operations"
  type = map(string)
  default = {
    create = "45m"
    update = "45m"
    delete = "45m"
  }
}

variable "maintenance_start_time" {
  description = "Start time for maintenance window"
  type        = string
  default     = "00:00"
}

variable "maintenance_end_time" {
  description = "End time for maintenance window"
  type        = string
  default     = "04:00"
}

variable "vnet_name" {
  description = "Name of the Azure Virtual Network"
  type        = string
  default     = "vnet-qovery-aks-test"
}

# variable "vnet_cidr" {
#   description = "CIDR block for the Virtual Network"
#   type        = string
# }

# variable "subnet_cidr" {
#   description = "CIDR block for the AKS subnet"
#   type        = string
# }

variable "enable_network_logging" {
  description = "Enable network flow logs"
  type        = bool
  default     = false
}

variable "log_analytics_workspace_resource_id" {
  description = "Resource ID of the Log Analytics workspace"
  type        = string
  default     = ""
}

variable "subscription_id" {
  description = "Azure subscription ID"
  type        = string
  default     = "{{ azure_subscription_id }}"
}

variable "tenant_id" {
  description = "Azure tenant ID"
  type        = string
  default     = "{{ azure_tenant_id }}"
}

variable "client_id" {
  description = "Azure client ID (service principal)"
  type        = string
  default     = "{{ azure_client_id }}"
}

variable "client_secret" {
  description = "Azure client secret (service principal)"
  type        = string
  sensitive   = true
  default     = "{{ azure_client_secret }}"
}

variable "vnet_cidr" {
  description = "CIDR block for the vnet cidr"
  type        = string
  default     = "10.0.0.0/16"
}

variable "node_cidr_zone_1" {
  description = "CIDR block for the node subnet"
  type        = string
  default     = "10.0.0.0/20"
}

variable "node_cidr_zone_2" {
  description = "CIDR block for the node subnet"
  type        = string
  default     = "10.0.16.0/20"
}

variable "node_cidr_zone_3" {
  description = "CIDR block for the node subnet"
  type        = string
  default     = "10.0.32.0/20"
}

variable "pod_cidr" {
  description = "CIDR block for the pod subnet"
  type        = string
  default     = "172.16.0.0/12"
}

variable "service_cidr" {
  description = "CIDR block for the service subnet"
  type        = string
  default     = "172.32.0.0/16"
}

variable "dns_service_ip" {
  description = "IP address for the DNS service"
  type        = string
  default     = "172.32.0.10"
}

locals {
  location         = var.location
  vnet_cidr        = var.vnet_cidr
  node_cidr_zone_1 = var.node_cidr_zone_1
  node_cidr_zone_2 = var.node_cidr_zone_2
  node_cidr_zone_3 = var.node_cidr_zone_3
  pod_cidr         = var.pod_cidr
  service_cidr     = var.service_cidr
  dns_service_ip   = var.dns_service_ip
  tags = {
    Environment = var.environment
    ManagedBy   = "Terraform"
    Project     = "Qovery"
  }
}

variable "environment" {
  description = "Environment name for tagging"
  type        = string
  default     = "dev"
}

variable "main_storage_account_name" {
  description = "Main storage account name"
  type        = string
  default     = "{{ main_storage_account_name }}"
}

variable "loki_namespace" {
  description = "Loki kubernetes namespace"
  type        = string
  default     = "{{ loki_namespace }}"
}

variable "cluster_sku_tier" {
  description = "Azure cluster SKU tier"
  type        = string
  default     = "{{ azure_cluster_sku_tier }}"
}

variable "load_balancer_sku_tier" {
  description = "Azure load balancer SKU tier"
  type        = string
  default     = "{{ azure_load_balancer_sku_tier }}"
}

variable "nat_gateway_sku_tier_name" {
  description = "Azure NAT SKU tier name"
  type        = string
  default     = "{{ azure_nat_gateway_sku_tier_name }}"
}


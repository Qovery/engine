# Azure Infrastructure Configuration

This directory contains Terraform configurations for deploying infrastructure on Microsoft Azure. The configuration includes an AKS cluster, networking, and associated resources.

## Components

- `aks_cluster.j2.tf`: Azure Kubernetes Service (AKS) cluster configuration
- `vnet_network.j2.tf`: Virtual Network and subnet configuration
- `providers.j2.tf`: Azure provider and backend configuration
- `variables.j2.tf`: Variable definitions for all resources

## Prerequisites

1. Azure subscription
2. Azure CLI installed and configured
3. Terraform >= 1.0
4. Service Principal with required permissions
5. Storage account for Terraform state

## Authentication

The configuration supports two authentication methods:
1. Service Principal (recommended for automation)
2. Azure CLI (for local development)

### Service Principal Setup

```bash
az ad sp create-for-rbac --name "Qovery-Terraform" --role Contributor
```

## Required Variables

- `subscription_id`: Azure subscription ID
- `tenant_id`: Azure tenant ID
- `resource_group_name`: Name of the resource group
- `location`: Azure region
- `kubernetes_cluster_name`: Name of the AKS cluster
- `vnet_name`: Name of the Virtual Network
- `vnet_cidr`: CIDR block for the Virtual Network
- `client_id`: Service Principal Application (client) ID
- `client_secret`: Service Principal secret
- `kubernetes_version`: Kubernetes version for the AKS cluster
- `node_cidr`: CIDR block for AKS nodes
- `pod_cidr`: CIDR block for Kubernetes pods
- `service_cidr`: CIDR block for Kubernetes services
- `dns_service_ip`: IP address for Kubernetes DNS service (within service_cidr range)

## Optional Variables

- `subnet_id`: Existing subnet ID (if you want to use an existing subnet)
- `enable_network_logging`: Enable network flow logs (default: false)
- `enable_monitoring`: Enable Azure Monitor for containers (default: true)
- `enable_secrets_encryption`: Enable Azure Key Vault Provider for Secrets Store CSI Driver (default: false)

## Usage

1. Initialize Terraform:
```bash
terraform init
```

2. Create a `config.tfvars` file with your variables:
```hcl
resource_group_name    = "rg-qovery-aks-test"
location               = "francecentral"
vnet_name              = "vnet-qovery-aks-test"
subscription_id        = "your-subscription-id"
tenant_id              = "your-tenant-id"

# Service Principal credentials
client_id              = "your-client-id"
client_secret          = "your-client-secret"

kubernetes_version     = "1.31.5"

# Network configuration
vnet_cidr              = "10.128.0.0/16"      # 65,536 addresses
node_cidr              = "10.128.0.0/20"      # 4,096 addresses for nodes
pod_cidr               = "172.16.0.0/12"       # 1,048,574 addresses for pods
service_cidr           = "10.129.0.0/16"      # 65,536 addresses for services
dns_service_ip         = "10.129.0.10"        # Within service_cidr
```

3. Apply the configuration:
```bash
terraform apply -var-file="config.tfvars"
```
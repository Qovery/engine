# Azure AKS Networking Architecture Summary

This document outlines the networking architecture design for our Azure-based AKS cluster. It details the integration of Azure services, Terraform resource configurations, and key considerations for scalability, security, and maintenance.

## Table of Contents
- [Architecture Overview](#architecture-overview)
- [Key Networking Components](#key-networking-components)
- [IP Address Management](#ip-address-management)
- [Security Controls](#security-controls)
- [Design Tradeoffs & Rationale](#design-tradeoffs--rationale)
- [Performance Optimization](#performance-optimization)
- [Network Monitoring](#network-monitoring)
- [Maintenance Best Practices](#maintenance-best-practices)

## Architecture Overview

Our AKS implementation leverages Azure CNI Powered by Cilium, Microsoft's recommended networking option for production workloads. This design offers enhanced security, improved performance, and simplified network policy management. Key integration points include:

- **Azure NAT Gateway:** Handles outbound traffic with a static public IP, ensuring controlled egress.
- **Dedicated CIDR Ranges:** Separate address spaces for nodes, pods, and services:
  - Nodes: `10.128.0.0/22`
  - Pods: `172.16.0.0/12`
  - Services: `10.129.0.0/16`
- **Virtual Network (VNet) Foundation:** Crafted with service endpoints and private link support to secure access to Azure resources.
- **Network Observability:** Built-in with Cilium to provide deep network insights and troubleshooting capabilities.

*Figure: Enhanced networking flow with Cilium, private endpoints, and improved observability.*

## Key Networking Components

### Terraform Resources

| Component                         | Purpose                | Key Configuration                                                    |
|-----------------------------------|------------------------|----------------------------------------------------------------------|
| `azurerm_kubernetes_cluster`      | AKS Cluster            | `network_plugin="azure"` <br> `network_plugin_mode="overlay"` <br> `network_policy="cilium"` <br> `outbound_type="userAssignedNATGateway"` |
| `azurerm_virtual_network`         | VNet Foundation        | `address_space=["10.128.0.0/16"]`                                     |
| `azurerm_nat_gateway`             | Egress Management      | Utilizes Standard SKU with a static public IP                        |
| `azurerm_subnet`                  | Node Network           | `address_prefixes=["10.128.0.0/22"]` <br> `service_endpoints=["Microsoft.Storage", "Microsoft.KeyVault", ...]` |
| `azurerm_private_endpoint`        | Secure Service Access  | For secure, private connectivity to Azure PaaS services              |

### Cilium-Powered Network Benefits

- **IP Conservation:** By using a non-VNet CIDR (172.16.0.0/12) for pods, the design preserves Azure's native IP space for other resources.
- **Enhanced Scalability:** Supports up to 1,048,574 pods compared to only 4,096 pods in a traditional Azure CNI setup.
- **Advanced Network Policies:** Cilium provides L3-L7 policy enforcement with enhanced visibility.
- **eBPF Technology:** Improves overall performance with efficient packet processing and routing.
- **Transparent Encryption:** Optional Wireguard-based encryption for pod-to-pod traffic.

## IP Address Management

| Network Layer | CIDR Range         | Address Count | Purpose                                   |
|---------------|--------------------|---------------|-------------------------------------------|
| VNet          | `10.128.0.0/16`    | 65,536        | Primary network space for all resources   |
| Nodes         | `10.128.0.0/22`    | 1,024         | Dedicated address space for AKS nodes     |
| Pods          | `172.16.0.0/12`    | 1,048,574     | Assigned for Kubernetes pod network       |
| Services      | `10.129.0.0/16`      | 65,536        | Cluster-internal communication for services |

**DNS Service IP:** `10.129.0.10` *(Static within service CIDR)*
terraform {
  required_version = "1.9.7"
  
  required_providers {
    azurerm = {
      source  = "hashicorp/azurerm"
      version = ">= 4.19"
    }
    azapi = {
      source = "azure/azapi"
      version = ">= 2.2.0"
    }
    time = {
      source  = "hashicorp/time"
      version = "0.9.0"
    }
  }
}

provider "azurerm" {
  features {}

  client_id       = var.client_id
  client_secret   = var.client_secret
  tenant_id       = var.tenant_id
  subscription_id = var.subscription_id
}
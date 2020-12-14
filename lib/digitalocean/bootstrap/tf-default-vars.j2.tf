# Qovery

variable "cloud_provider" {
  description = "Cloud provider name"
  default = "do"
  type = string
}

variable "region" {
  description = "AWS region to store terraform state and lock"
  default     = "{{ do_region }}"
  type        = string
}

variable "organization_id" {
  description = "Qovery Organization ID"
  default     = "{{ organization_id }}"
  type        = string
}

variable "qovery_nats_url" {
  description = "URL of qovery nats server"
  default = "{{ qovery_nats_url }}"
  type = string
}

variable "test_cluster" {
  description = "Is this a test cluster?"
  default = "{{ test_cluster }}"
  type = string
}

// kubernetes first worker cluster
variable "doks_master_name" {
  default = "{{ oks_master_name }}"
  type    = string
}

variable "doks_version" {
  default = "{{ oks_version }}"
  type    = string
}

variable "doks_cluster_id" {
  default = "{{ oks_cluster_id }}"
  type    = string
}

variable "doks_master_size" {
  default = "{{ oks_master_size }}"
  type    = string
}

variable "doks_master_node_count" {
  default = 5
  type = number
}

variable "doks_master_autoscale" {
  default = true
  type    = bool
}

// kubernetes WORKER second cluster
variable "doks_pool_name" {
  default = "{{ oks_master_name }}"
  type    = string
}

variable "doks_pool_autoscale" {
  default = true
  type    = bool
}

// for vpc segmentation see vpc.tf
variable "cidr_block" {
  description = "CIDR block for VPC segementation"
  default = "{{ vpc_cidr_block }}"
}

variable "vpc_name" {
  description = "name of vpc, take care to insert unique names"
  default = "{{ vpc_name }}"
}

variable "space_bucket_kubeconfig" {
  description = "space bucket with kubeconfigs"
  default = "{{ space_bucket_kubeconfig }}"
}

variable "space_access_id" {
  description = "credentials space access key"
  default = "{{ spaces_access_id }}"
}

variable "space_secret_key" {
  description = "credentials space access key"
  default = "{{ spaces_secret_key }}"
}

# Engine info
variable "qovery_engine_info" {
  description = "Qovery engine info"
  default = {
    "token" = "{{ engine_version_controller_token }}"
    "api_fqdn" = "{{ qovery_api_url }}"
  }
  type = map(string)
}

variable "qovery_engine_replicas" {
  description = "This variable is used to get random ID generated for the engine"
  default = "2"
  type = number
}

# Agent info

variable "qovery_agent_info" {
  description = "Qovery agent info"
  default = {
    "token" = "{{ agent_version_controller_token }}"
    "api_fqdn" = "{{ qovery_api_url }}"
  }
  type = map(string)
}

variable "qovery_agent_replicas" {
  description = "This variable is used to get random ID generated for the agent"
  default = "1"
  type = number
}

# Helm alert manager discord
variable "discord_api_key" {
  description = "discord url with token for used for alerting"
  default = "{{ discord_api_key }}"
  type = string
}

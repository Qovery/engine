// global vars used everywhere
variable "digitalocean_region" {
  default = "{{ do_region }}"
  type    = string
}

// container registry
variable "container_registry_name" {
  default = "qovery"
  type = string
}

// kubernetes MASTER cluster
variable "kubernetes_master_cluster_name" {
  default = "{{ kubernetes_master_cluster_name }}"
  type    = string
}

variable "oks_version" {
  default = "{{ oks_version }}"
  type    = string
}

variable "oks_cluster_id" {
  default = "{{ oks_cluster_id }}"
  type    = string
}

variable "oks_master_name" {
  default = "{{ kubernetes_master_cluster_name }}"
  type    = string
}

variable "oks_master_size" {
  default = "{{ oks_master_size }}"
  type    = string
}

variable "oks_master_node_count" {
  default = 2
}

variable "oks_master_autoscale" {
  default = false
  type    = bool
}

// kubernetes WORKER cluster
variable "oks_pool_name" {
  default = "{{ kubernetes_master_cluster_name }}"
  type    = string
}

variable "oks_pool_autoscale" {
  default = false
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
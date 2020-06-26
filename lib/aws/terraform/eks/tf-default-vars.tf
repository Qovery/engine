# AWS specific
variable "cloud_provider" {
  description = "Cloud provider name"
  default = "aws"
  type = string
}

variable "region" {
  description = "AWS region to store terraform state and lock"
  default     = "tbd"
  type        = string
}

variable "vpc_cidr_block" {
  description = "VPC CIDR block"
  default = "10.0.0.0/16"
  type = string
}

variable "test_cluster" {
  description = "Is this a test cluster?"
  default = "false"
  type = string
}

# Kubernetes

variable "k8s_versions" {
  description = "Kubernetes version"
  default = {
    "masters": "1.14",
    "workers": "1.14",
  }
  type = map(string)
}

variable "region_cluster_name" {
  description = "Kubernetes cluster name with region"
  default     = "tbd"
  type        = string
}

variable "cluster_name" {
  description = "Kubernetes cluster name"
  default     = "tbd"
  type        = string
}

variable "k8s_access_cidr_blocks" {
  description = "Kubernetes cluster name"
  default     = ["185.162.179.5/32", "78.192.247.93/32"]
  type        = list(string)
}

variable "k8s_nb_subnets_per_zone" {
  description = "Kubernetes workers, number of desired subnets (3 zones used)"
  default     = 21
  type        = number
}

variable "k8s_cidr_subnet" {
  description = "Kubernetes workers CIDR (x.x.x.x/CIDR)"
  default     = 23
  type        = number
}

variable "k8s-workers" {
  description = "Kubernetes workers type"
  default = {
    "instance-type": "tbd",
    "ami": "tbd",
    "min-size": "tbd",
    "max-size": "tbd",
    "desired-capacity": "tbd"
  }
  type = map(string)
}
# AWS specific
variable "cloud_provider" {
  description = "Cloud provider name"
  default = "aws"
  type = string
}

variable "region" {
  description = "AWS region to store terraform state and lock"
  default     = "{{ aws_region }}"
  type        = string
}

variable "vpc_cidr_block" {
  description = "VPC CIDR block"
  default = "{{ vpc_cidr_block }}"
  type = string
}

variable "test_cluster" {
  description = "Is this a test cluster?"
  default = "false"
  type = string
}

# Kubernetes

variable "eks-subnets-zone-a" {
  description = "EKS subnets Zone A"
  default = {{ eks_zone_a_subnet_blocks }}
  type = list(string)
}

variable "eks-subnets-zone-b" {
  description = "EKS subnets Zone B"
  default = {{ eks_zone_b_subnet_blocks }}
  type = list(string)
}

variable "eks-subnets-zone-c" {
  description = "EKS subnets Zone C"
  default = {{ eks_zone_c_subnet_blocks }}
  type = list(string)
}

variable "eks_cidr_subnet" {
  description = "EKS CIDR (x.x.x.x/CIDR)"
  default     = {{ eks_cidr_subnet }}
  type        = number
}

variable "k8s_versions" {
  description = "Kubernetes version"
  default = {
    "masters": "{{ eks_masters_version }}",
    "workers": "{{ eks_workers_version }}",
  }
  type = map(string)
}

variable "region_cluster_name" {
  description = "Kubernetes cluster name with region"
  default     = "{{ region_cluster_id }}"
  type        = string
}

variable "cluster_name" {
  description = "Kubernetes cluster name"
  default     = "{{ eks_cluster_name }}"
  type        = string
}

variable "cluster_id" {
  description = "Qovery cluster ID"
  default = "{{ region_cluster_id }}"
  type = string
}

variable "k8s_access_cidr_blocks" {
  description = "Kubernetes cluster name"
  default     = ["185.162.179.5/32", "78.192.247.93/32"]
  type        = list(string)
}

# S3 bucket name

variable "s3_bucket_kubeconfig" {
  description = "S3 bucket with kubeconfigs"
  default = "{{ region_cluster_id }}"
  type = string
}

# EC2 SSH default SSH key

variable "ec2_ssh_default_key" {
  description = "Default SSH key"
  default = {
    "key_name" = "{{ region_cluster_id }}-qovery"
    "public_key" = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQCnsfaJtod3fuSnE2zqfw+I6l696ipy18utqXpQOTzw0lT4y+CQCyVrR3og54RGwERoOT7KnoneyWzJMEzC+58mXDqe7oM7HgVgYOlEwYuFPO7EZBGaDWFoKMMzgFgdVyEVkoKE/s/2ClOqLvBt7Qq+Z8yQWrxjlluHncXSE6aNoog+Ard2qQhhZOGzwS2uGarkNj11x7e5qQ6kZcwQz+1LSJzTHfn6yK8RhvTDwhmYBy6kYfG+IYacUqToeqkFOiTbdmhntFYRf7J+0N3tVt8s3VUoLAg3uD2ycEqRG48WybAj+VLJHLC31iBrqvNRQqPfubM2ss7Qhv96nOnqMhNh pmavro@deb-pmavro"
  }
  type = map(string)
}

# RDS

variable "rds_nb_subnets_per_zone" {
  description = "RDS number of desired subnets (3 zones used)"
  default     = 7
  type        = number
}

variable "rds_cidr_subnet" {
  description = "RDS CIDR (x.x.x.x/CIDR)"
  default     = 23
  type        = number
}

# Elasticsearch

variable "es_nodes_number" {
  description = "Number of Elasticsearch nodes"
  default = 3
  type = number
}

variable "es_volume_size" {
  description = "Disk size per node"
  default = 50
  type = number
}

variable "es_nb_subnets_per_zone" {
  description = "Elasticsearch number of desired subnets (3 zones used)"
  default     = 2
  type        = number
}

variable "es_cidr_subnet" {
  description = "Elasticsearch CIDR (x.x.x.x/CIDR)"
  default     = 23
  type        = number
}

variable "es-logs-curator" {
  description = "Curator config"
  default = {
    "cron": "0 0 * * *"
    "days_to_keep": "7"
  }
  type = map(string)
}
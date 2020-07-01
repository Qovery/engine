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

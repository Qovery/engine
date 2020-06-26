# This data source is included for ease of sample architecture deployment
# and can be swapped out as necessary.
data "aws_availability_zones" "available" {}

resource "aws_vpc" "eks" {
  cidr_block = var.vpc_cidr_block
  enable_dns_hostnames = true

  tags = map(
  "Name", "${var.region_cluster_name}-workers",
  "kubernetes.io/cluster/${var.region_cluster_name}", "shared",
  "kubernetes.io/role/elb", 1,
  "ClusterName", var.cluster_name,
  "RegionClusterName", var.region_cluster_name,
  "Region", var.region,
  )
}

resource "aws_subnet" "eks-zone-a" {
  count = var.k8s_nb_subnets_per_zone

  availability_zone = data.aws_availability_zones.available.names[0]
  cidr_block = "10.0.${count.index * 2}.0/${var.k8s_cidr_subnet}"
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = true
  tags = aws_vpc.eks.tags
}

resource "aws_subnet" "eks-zone-b" {
  count = var.k8s_nb_subnets_per_zone

  availability_zone = data.aws_availability_zones.available.names[1]
  cidr_block = "10.0.${count.index * 2 + 42}.0/${var.k8s_cidr_subnet}"
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = true
  tags = aws_vpc.eks.tags
}

resource "aws_subnet" "eks-zone-c" {
  count = var.k8s_nb_subnets_per_zone

  availability_zone = data.aws_availability_zones.available.names[2]
  cidr_block = "10.0.${count.index * 2 + 84}.0/${var.k8s_cidr_subnet}"
  vpc_id = aws_vpc.eks.id
  map_public_ip_on_launch = true
  tags = aws_vpc.eks.tags
}

resource "aws_internet_gateway" "eks_cluster" {
  vpc_id = aws_vpc.eks.id

  tags = {
    ClusterName = var.cluster_name
    RegionClusterName = var.region_cluster_name
    Region = var.region
  }
}

resource "aws_route_table" "eks_cluster" {
  vpc_id = aws_vpc.eks.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.eks_cluster.id
  }
}

resource "aws_route_table_association" "eks_cluster-zone-a" {
  count = var.k8s_nb_subnets_per_zone

  subnet_id = aws_subnet.eks-zone-a.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "eks_cluster-zone-b" {
  count = var.k8s_nb_subnets_per_zone

  subnet_id = aws_subnet.eks-zone-b.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

resource "aws_route_table_association" "eks_cluster-zone-c" {
  count = var.k8s_nb_subnets_per_zone

  subnet_id = aws_subnet.eks-zone-c.*.id[count.index]
  route_table_id = aws_route_table.eks_cluster.id
}

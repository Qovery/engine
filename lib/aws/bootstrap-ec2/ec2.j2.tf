data "aws_ami" "debian" {
  most_recent = true

  filter {
    name   = "name"
    values = [var.ec2_image_info.name]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }

  # to get owner id:
  # aws ec2 describe-images --image-ids <ami-id> --region us-west-2 | jq -r '.Images[0].OwnerId'
  owners = [var.ec2_image_info.owners]
}

resource "aws_instance" "web" {
  ami           = data.aws_ami.debian.id
  instance_type = var.ec2_instance.instance_type

  # disk
  root_block_device {
    volume_size = "30" # GiB
    volume_type = "gp2"
    encrypted = true
  }

  # network
  associate_public_ip_address = true

  # security
  #vpc_security_group_ids = [aws_vpc.ec2.*.id]

  user_data = local.bootstrap

  tags = {
    Name = "HelloWorld"
  }
}

locals {
  bootstrap = <<BOOTSTRAP
#!/bin/bash
apt-get update
apt-get -y install curl s3cmd

export INSTALL_K3S_VERSION=${var.k3s_config.version}
export INSTALL_K3S_CHANNEL=${var.k3s_config.channel}
export INSTALL_K3S_EXEC="${var.k3s_config.exec}"
curl -sfL https://get.k3s.io | sh -
echo 'export KUBECONFIG=/etc/rancher/k3s/k3s.yaml' >> /etc/profile

while [ ! -f /etc/rancher/k3s/k3s.yaml ] ; do
    echo "kubeconfig is not yet present, sleeping"
    sleep 1
done
s3cmd --access_key={{ aws_access_key }} --secret_key={{ aws_secret_key }} --region={{ aws_region }} put /etc/rancher/k3s/k3s.yaml s3://${var.s3_bucket_kubeconfig}/${var.kubernetes_cluster_id}.yaml
BOOTSTRAP
}
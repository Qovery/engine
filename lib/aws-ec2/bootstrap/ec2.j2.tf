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

resource "aws_key_pair" "qovery_ssh_key" {
  key_name   = "qovery-key"
  public_key = "{{ qovery_ssh_key }}"
}

resource "aws_ebs_volume" "ebs_disk1" {
  availability_zone = aws_subnet.ec2_zone_a[0].availability_zone
  size              = var.ec2_instance.disk_size_in_gb
  type              = "gp2"
  encrypted         = true
  tags              = local.tags_common
}

resource "aws_volume_attachment" "ebs_disk1" {
  device_name = "/dev/sdq"
  volume_id    = aws_ebs_volume.ebs_disk1.id
  instance_id  = aws_instance.ec2_instance.id
  force_detach = true
}

resource "aws_instance" "ec2_instance" {
  ami           = data.aws_ami.debian.id
  instance_type = var.ec2_instance.instance_type

  # disk
  root_block_device {
    volume_size = var.ec2_instance.disk_size_in_gb # GiB
    volume_type = "gp2"
    encrypted = true
  }

  # network
  associate_public_ip_address = true

  # security
  vpc_security_group_ids = [aws_security_group.ec2_instance.id]
  subnet_id = aws_subnet.ec2_zone_a[0].id

  # ssh
  key_name = aws_key_pair.qovery_ssh_key.key_name

  # k3s install
  user_data = local.bootstrap
  user_data_replace_on_change = true

  tags = merge(
      local.tags_common,
      {
        "Service" = "EC2"
      }
    )

  depends_on = [
    aws_s3_bucket.kubeconfigs_bucket,
    aws_ebs_volume.ebs_disk1
  ]
}

resource "time_static" "on_ec2_create" {}

locals {
  bootstrap = <<BOOTSTRAP
#!/bin/bash

function print_title() {
  echo -e "\n######### $1 #########\n"
}

# enable logs to file and console
exec > >(tee ${var.ec2_instance.user_data_logs_path}|logger -t user-data -s 2>/dev/console) 2>&1

export KUBECONFIG_FILENAME="${var.kubernetes_cluster_id}.yaml"
export NEW_KUBECONFIG_PATH="/tmp/$KUBECONFIG_FILENAME"

print_title "Install packages"
apt-get update
apt-get -y install curl s3cmd parted

print_title "Prepare Rancher dedicated data disk"
disk_device="$(lsblk -r | grep disk | grep ${var.ec2_instance.disk_size_in_gb}G | tail -1 | awk '{ print $1 }')"
echo "disk_device: $disk_device"
disk_device_path="/dev/$disk_device"
echo "disk_device_path: $disk_device_path"
if [ $(lsblk -r | grep $disk_device | grep part | grep -c $disk_device) -eq 0 ] ; then
  echo "No partition found, erasing disk"
  parted -s -a optimal $disk_device_path mklabel gpt
  parted -s -a optimal $disk_device_path mkpart primary ext4 0% 100%
  partprobe $disk_device_path
  sleep 5
  export partition_path="/dev/$(lsblk -r | grep $disk_device | awk '/part/{print $1}')"
  echo "partition_path: $partition_path"
  mkfs.ext4 $partition_path
  sleep 2
else
  echo "Partition already exists, not erasing"
fi
echo "$partition_path /var/lib/rancher ext4 rw,discard 0 0" >> /etc/fstab
mkdir -p /var/lib/rancher
mount /var/lib/rancher
sleep 2
if [ $(df | grep -c '/var/lib/rancher') -eq 0 ] ; then
  echo "No data disk was able to be mounted, can't continue"
  exit 1
fi

print_title "Install k3s"
export INSTALL_K3S_VERSION=${var.k3s_config.version}
export INSTALL_K3S_CHANNEL=${var.k3s_config.channel}
export INSTALL_K3S_EXEC="--https-listen-port=${random_integer.kubernetes_external_port.result} ${var.k3s_config.exec}"
echo "k3s agrs: $INSTALL_K3S_EXEC"
curl -fL https://get.k3s.io | sh -
echo 'export KUBECONFIG=/etc/rancher/k3s/k3s.yaml' >> /etc/profile

while [ ! -f /etc/rancher/k3s/k3s.yaml ] ; do
    echo "kubeconfig is not yet present, sleeping"
    sleep 1
done

print_title "Push Kubeconfig to S3"
# Calico will be installed and metadata won't be accessible anymore, it can only be done during bootstrap
public_hostname="$(curl -s http://169.254.169.254/latest/meta-data/public-hostname)"
sed "s/127.0.0.1/$public_hostname/g" /etc/rancher/k3s/k3s.yaml > $NEW_KUBECONFIG_PATH
sed -i "s/:6443/:${random_integer.kubernetes_external_port.result}/g" $NEW_KUBECONFIG_PATH
s3cmd --access_key={{ aws_access_key }} --secret_key={{ aws_secret_key }} --region={{ aws_region }} put $NEW_KUBECONFIG_PATH s3://${var.s3_bucket_kubeconfig}/$KUBECONFIG_FILENAME
rm -f $NEW_KUBECONFIG_PATH
BOOTSTRAP
}
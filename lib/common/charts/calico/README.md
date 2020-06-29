# Calico

We use it only for NetworkPolicy

# This is exactly the recommended gke configuration:

https://github.com/aws/amazon-vpc-cni-k8s/blob/master/config/v1.5/calico.yaml

# to install it:

helm install calico . --namespace=kube-system

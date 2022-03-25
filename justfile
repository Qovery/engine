set dotenv-load := false

_default:
    @just --list

#####################
# FOR TESTS
#####################
kube_cluster_name := "kube-test-cluster"

spawn_kube_cluster $DOCKER_HOST="":
  if [ -z $DOCKER_HOST ]; then unset $DOCKER_HOST; fi
  docker run -d --rm -p 5000:5000 --name engine-registry registry:2
  k3d cluster create -a 0 \
    --image rancher/k3s:v1.21.10-k3s1 \
    --no-lb \
    --k3s-arg "--disable=traefik" \
    --wait {{kube_cluster_name}} || k3d cluster start --wait {{kube_cluster_name}}

  kubectl -n kube-system wait pod --for=condition=Ready --selector app=local-path-provisioner || sleep 30
  kubectl -n kube-system wait pod --for=condition=Ready --selector app=local-path-provisioner

destroy_kube_cluster $DOCKER_HOST="":
  if [ -z $DOCKER_HOST ]; then unset $DOCKER_HOST; fi
  -docker kill engine-registry
  -k3d cluster delete {{kube_cluster_name}}


#####################
# TESTS
#####################

test_local_stack $DOCKER_HOST="": (spawn_kube_cluster DOCKER_HOST)
  #!/usr/bin/env bash
  set -euxo pipefail
  echo "==========================TEST WITH LOCAL STACK==========================="
  trap "just destroy_kube_cluster $DOCKER_HOST" EXIT
  if [ -z $DOCKER_HOST ]; then unset $DOCKER_HOST; fi
  cargo test --manifest-path cloned-engine/Cargo.toml --features test-all-local

test $DOCKER_HOST="": (test_local_stack DOCKER_HOST) 


#####################
# HELPERS
#####################

clippy_check:
  cargo clippy --locked --all --all-features -- -D warnings

linter_fix:
    cargo fmt --all
    cargo clippy --fix --locked --all --all-features --lib -- -D warnings
    cargo fmt --all


FROM hashicorp/terraform:{{provider_version}} AS terraform

FROM debian:trixie-slim

COPY --from=terraform /bin/terraform /usr/local/bin/terraform

RUN <<EOF
set -e
apt-get update
apt-get install -y --no-install-recommends dumb-init rsync bash ca-certificates
rm -rf /var/lib/apt/lists/*
useradd -m -u 1000 app
mkdir /data
chown -R app:app /data
EOF

WORKDIR /data
COPY --chown=app:app . .

# Custom build fragment (injected from user defined content)
#{{custom_fragment}}

RUN chmod +x entrypoint.sh
USER app

ENTRYPOINT ["/usr/bin/dumb-init", "--", "/bin/sh", "/data/entrypoint.sh"]

FROM hashicorp/terraform:{{provider_version}}

RUN <<EOF
set -e
apk update
apk add dumb-init rsync
adduser -D -u 1000 app
mkdir /data
chown -R app:app /data
EOF

WORKDIR /data
COPY --chown=app:app . .

# Custom build fragment (injected from qovery-build-fragment.dockerfile if present)
{{custom_fragment}}

RUN chmod +x entrypoint.sh
USER app

ENTRYPOINT ["/usr/bin/dumb-init", "--", "/bin/sh", "/data/entrypoint.sh"]

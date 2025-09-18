# ExternalDNS Secret Chart

A Helm chart for managing ExternalDNS secrets and credentials for various DNS providers. This chart is specifically designed to create and manage secrets that are consumed by ExternalDNS for DNS provider authentication.

## Overview

This chart creates Kubernetes secrets containing DNS provider credentials that ExternalDNS uses for authentication. Currently supports PowerDNS (PDNS) and Cloudflare API key management, with the ability to extend support for additional DNS providers.

## Prerequisites

- Kubernetes 1.19+
- Helm 3.2.0+

## Installation

### Installing the Chart

To install the chart with the release name `external-dns-secret`:

```bash
helm install external-dns-secret ./external-dns-secret
```

### Installing with PowerDNS Configuration

```bash
helm install external-dns-secret ./external-dns-secret \
  --set pdns.apiKey="your-powerdns-api-key"
```

## Configuration

### Parameters

| Name                  | Description                                        | Value |
| --------------------- | -------------------------------------------------- | ----- |
| `nameOverride`        | String to partially override the release name      | `""`  |
| `fullnameOverride`    | String to fully override the release name          | `""`  |
| `pdns.apiKey`         | PowerDNS API key for authentication                | `""`  |
| `cloudflare.apiToken` | Cloudflare API token (recommended method)          | `""`  |
| `cloudflare.apiKey`   | Cloudflare API key (legacy method, requires email) | `""`  |

### PowerDNS Configuration

To configure PowerDNS credentials, set the following values:

```yaml
pdns:
  apiKey: "your-powerdns-api-key-here"
```

### Cloudflare Configuration

To configure Cloudflare credentials, you have two options:

**Option 1: API Token (Recommended)**

```yaml
cloudflare:
  apiToken: "your-cloudflare-api-token-here"
```

**Option 2: API Key (Legacy)**

```yaml
cloudflare:
  apiKey: "your-cloudflare-api-key-here"
```

Note: When using the API key method, you must also configure the email in your ExternalDNS deployment separately.

## Usage

### With ExternalDNS

This chart is designed to work with ExternalDNS. After deploying this chart, configure your ExternalDNS deployment to reference the created secret:

```yaml
# ExternalDNS configuration example for PowerDNS
apiVersion: apps/v1
kind: Deployment
metadata:
  name: external-dns
spec:
  template:
    spec:
      containers:
        - name: external-dns
          env:
            - name: PDNS_API_KEY
              valueFrom:
                secretKeyRef:
                  name: external-dns-secret # Name created by this chart
                  key: pdns_api_key

# ExternalDNS configuration example for Cloudflare
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: external-dns
spec:
  template:
    spec:
      containers:
        - name: external-dns
          env:
            - name: CF_API_TOKEN
              valueFrom:
                secretKeyRef:
                  name: external-dns-secret # Name created by this chart
                  key: cloudflare_api_token
            # OR for legacy API key method:
            - name: CF_API_KEY
              valueFrom:
                secretKeyRef:
                  name: external-dns-secret
                  key: cloudflare_api_key
```

### Secret Structure

The chart creates a Kubernetes Secret with the following structure:

**For PowerDNS:**

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: external-dns-secret
type: Opaque
data:
  pdns_api_key: <base64-encoded-api-key>
```

**For Cloudflare:**

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: external-dns-secret
type: Opaque
data:
  cloudflare_api_token: <base64-encoded-api-token>
  # OR for legacy method:
  cloudflare_api_key: <base64-encoded-api-key>
```

## Examples

### Basic Installation

```bash
# Install with PowerDNS API key
helm install my-external-dns-secret ./external-dns-secret \
  --set pdns.apiKey="my-secret-api-key"

# Install with Cloudflare API token
helm install my-external-dns-secret ./external-dns-secret \
  --set cloudflare.apiToken="my-cloudflare-api-token"

# Install with Cloudflare API key (legacy)
helm install my-external-dns-secret ./external-dns-secret \
  --set cloudflare.apiKey="my-cloudflare-api-key"
```

### Using Values File

Create a `values.yaml` file:

**For PowerDNS:**

```yaml
pdns:
  apiKey: "your-powerdns-api-key"

nameOverride: "my-dns-secret"
```

**For Cloudflare:**

```yaml
cloudflare:
  apiToken: "your-cloudflare-api-token"

nameOverride: "my-dns-secret"
```

Then install:

```bash
helm install my-external-dns-secret ./external-dns-secret -f values.yaml
```

## Security Considerations

- **Never commit API keys to version control**
- Use Kubernetes secrets or external secret management systems
- Consider using tools like Sealed Secrets, External Secrets Operator, or Vault for production deployments
- Rotate API keys regularly

## Integration with Qovery Engine

This chart is part of the Qovery Engine infrastructure and is automatically deployed when ExternalDNS is configured for PowerDNS providers. The chart integrates with:

- Qovery's DNS management system
- ExternalDNS deployments managed by the engine
- Cloud provider DNS configurations

## Troubleshooting

### Common Issues

1. **Secret not created**: Ensure `pdns.apiKey` is set and not empty
2. **ExternalDNS can't authenticate**: Verify the API key is correct and has proper permissions
3. **Permission errors**: Ensure the PowerDNS API key has sufficient permissions for DNS record management

### Debugging

Check if the secret was created:

```bash
kubectl get secrets -n <namespace>
kubectl describe secret external-dns-secret -n <namespace>
```

Verify the secret contents (be careful with sensitive data):

```bash
kubectl get secret external-dns-secret -n <namespace> -o yaml
```

## Development

### Extending for Additional Providers

To add support for additional DNS providers, extend the `values.yaml` and `templates/secret.yaml`:

1. Add provider configuration to `values.yaml`
2. Update `templates/secret.yaml` to include new provider credentials
3. Update this README with new configuration options

The chart now supports multiple DNS providers including PowerDNS and Cloudflare. To add support for additional providers, follow the same pattern by extending the `values.yaml` and `templates/secret.yaml` files.

## Contributing

This chart is maintained as part of the Qovery Engine project. For contributions:

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Test thoroughly
5. Submit a pull request

## License

Copyright © 2025 Qovery. All rights reserved.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

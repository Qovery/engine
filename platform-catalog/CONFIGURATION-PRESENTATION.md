# Configuration presentation

A component may display selected top-level configuration fields owned by another component in the **same layer**:

```yaml
- key: karpenter-crd
  # Existing chart, release and configRef stay unchanged.
  configurationSections:
    - sourceComponentKey: karpenter-configuration
      fieldKeys: [resources]
```

The destination retains its own configuration and cluster inputs. The Console renders the declared section using the source component's field descriptors and resolver. The selected fields are omitted from the source's native editor, but remain in its descriptor, stored configuration and Helm output. Other source fields and its cluster inputs remain accessible in the source editor.

This is presentation metadata, not a dependency edge or a data migration. It does not change component keys, releases, deployment order, validation scope, or ownership of Kubernetes objects. In particular, raw Karpenter YAML still belongs to `karpenter-configuration` / Helm release `karpenter-custom-configuration`, not to the CRD chart.

q-core rejects unknown or self-referencing sources, cross-layer references, empty/duplicate field assignments and fields absent from the source's DESCRIBE contract in an applicable context. Each source may appear once per destination; each field may have one destination. Sections reference top-level fields, not JSON paths. Shared field types and Blueprint are unchanged.

## Delivery

1. Merge and deploy q-core support before publishing this template: old q-core parsers reject unknown manifest properties.
2. Merge OpenAPI and publish the generated TypeScript client; update the Console dependency and ship its generic renderer.
3. Publish and activate the catalogue. Existing bindings need no migration.

Consumers without section support can continue rendering every field under its source. Catalogues without sections keep their existing behavior in the generic Console. On the temporary front-only POC, the Karpenter-specific routing continues until replaced.

The source resolver still validates the whole source configuration. Independent section validation, shared cluster inputs, full CRD validation and resource readiness monitoring are separate work.

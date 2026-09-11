# Karpenter activation and release identities

This template separates two optional, disabled-by-default layers:

| Layer | Components | Helm releases in kube-system |
| --- | --- | --- |
| `karpenter` | `karpenter-crd`, `karpenter` | `karpenter-crd`, `karpenter` |
| `karpenter-custom-configuration` | `karpenter-configuration` | `karpenter-custom-configuration` |

The custom configuration keeps its existing component/config-bundle key and source-2 schema. It requires the controller and CRDs, but the controller does not require custom pools. Workloads keep their `after: karpenter-configuration` edges: these order enabled units without forcing the custom pool layer to be enabled. With controller only, workloads may therefore use independently existing pools; actual cluster capacity remains an operational prerequisite.

## Legacy preservation and migration boundary

The custom chart MUST NOT use the legacy `kube-system/karpenter-configuration` Helm release. An upgrade of that release with a chart rendering only newly supplied pools removes resources from its previous manifest. The distinct custom release prevents that implicit Helm upgrade. The controller-only plan contains no unit targeting either configuration release and performs no migration or adoption of existing NodePools.

This change does not restore resources already deleted during a previous migration. It also does not move resources already owned by the legacy release into the custom release. Installing custom pools whose Kubernetes names already exist under another release is unsupported without an explicit migration decision; retain Helm ownership checks and do not use takeover/force flags. Existing installations of the former v2 chart under the legacy name need a separately reviewed inventory and recovery/migration plan before enabling the custom layer.

The future `karpenter-qovery-configuration` component is a separate, unimplemented step. Its intended ownership is the Qovery default/stable pools and legacy `karpenter-configuration` release. Reusing that identity must preserve every managed resource and existing setting or explicitly refuse unsupported migration; a new template cannot simply replace the legacy manifest. No default/stable/cronjob resources or associated UI parameters are added here.

## q-core and drafts

No q-core production/API change is necessary: optional layers, `requires`, `after` and selection-based VALIDATE/COMPILE already implement this contract. An explicitly retained invalid managedConfig draft is still validated even when its layer is disabled. For a controller-only test, do not submit a new empty/invalid NodePool draft; removing a rejected local draft from the request is different from deleting live cluster resources. Never silently discard a saved valid configuration.

This catalog change also restores the controller CPU request to the legacy value of 100m, retaining its 1Gi memory request and limit. Publish the updated Karpenter configuration bundle and template together through publish-platform-catalog; no Engine binary change is required. Publication does not migrate existing Helm resources.

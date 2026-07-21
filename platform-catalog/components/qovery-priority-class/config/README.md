# qovery-priority-class

This bundle reproduces the priority-class portion of the legacy BYOK stack.
It has no cluster-specific value and therefore no runtime input or evaluator.

The component belongs to the mandatory `qovery-stack` layer with the cluster and shell agents, for
both Qovery-managed and customer-managed clusters. It has no dependency. Components that select one
of these priority classes must declare their dependency explicitly in the root template so q-core
orders the PriorityClass before their Helm release.

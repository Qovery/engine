# qovery-priority-class

This bundle reproduces the priority-class portion of the legacy BYOK stack.
It has no cluster-specific value and therefore no runtime input or evaluator.

The `cluster-foundation` layer is customer-managed, optional, and disabled by
default. The component has no dependency. Components that select one of these
priority classes must declare their dependency explicitly in the root template.

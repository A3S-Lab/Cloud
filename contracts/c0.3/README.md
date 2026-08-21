# C0.3 notification contracts

`outbound-notification-subscription-v2.acl` is the canonical user-configurable
provider-attempt budget contract. The immutable definition pins one exact
Connector revision and `maximum_provider_attempts` from 1 through 8.

The historic v1 schema remains valid and always means eight attempts. Changing
any subscription setting creates a new definition; revoke remains the only
mutation. Delivery progress comes only from A3S Event redelivery and immutable
C6 Connector evidence, not from another counter, timer, queue, or scheduler.

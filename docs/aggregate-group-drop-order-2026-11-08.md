# Aggregate group error-path owner order

Serial and worker-partial aggregate maps now share a structural owner with
their existing GroupMemory reservation. On early return the map and its
groups are dropped before that reservation. Synchronized test-only hooks
exercise cancellation after an actual serial row insertion and partial
accumulator update; they observe cleanup, not individual Value destructors.

These checks can report cancellation before the previous post-mutation
admission or resize error. The repair does not pre-admit a mutation's new
allocations or give bare successful aggregate output rows a retained owner.
Group key creation, serializer scratch and other aggregate consumers remain
separate gaps. Probes are authored first and remain unexecuted until the
consolidated validation gate.

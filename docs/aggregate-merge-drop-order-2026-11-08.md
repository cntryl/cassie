# Aggregate merge error-path owner order

The merged partial-group map is held before the partition and incremental
merge reservations in one owner, so errors destroy the map before those
reservations. A synchronized test-only hook after an occupied-group mutation
checks the cancellation cleanup path. It observes cancellation and reservation
release, not an individual Value destructor.

This partial repair does not pre-admit allocations made inside accumulator
merge before its growth is measured. Existing successful output conversion
still returns bare BatchRows after the group reservations retire; that
handoff requires separate ownership work. Partition/serial state drop order,
serializer scratch and all other aggregate ownership remain separate gaps.
The added interruption check after a successful merge can report cancellation
before the old post-mutation growth admission. Probes are authored first and
remain unexecuted until the consolidated validation gate.

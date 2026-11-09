# Statistical retained NUMERIC sizing

The #806 variance/stddev accumulator sizes its exact sum and sum-of-squares
after transitions and partial-state merges. Its prior size helper cloned the
complete NUMERIC value only to invoke `TypedValue::heap_bytes`. That method
uses the finite mantissa's magnitude bit length, rounded up to bytes, and zero
for non-finite values. The clone was an additional unadmitted allocation.

Read the same bit count directly through the borrowed NUMERIC instead. This
preserves the prior accounting estimate, arithmetic, output and error ordering;
it does not claim that the estimate equals allocator capacity. In particular,
large-integer transition, merge, finalization, result ownership and cancellation
inside bigint operations remain independent open requirements. Byte-boundary,
special-value and real transition/merge probes are authored but unexecuted.

An exact/exact partial-state merge also cloned the entire right-hand NUMERIC
state before adding its borrowed fields. The selected follow-up borrows that
state only for this branch. The old replace-before-merge behavior is retained:
on error the left moments are empty and the result family was already set.
Empty and mixed-family paths retain their existing cloning and conversion.
The source partial and its owner must remain alive for the complete merge;
the BigInt additions themselves still have the preceding resource limitations.

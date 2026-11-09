# Serial aggregate accumulator error-path owner order

The serial accumulator's existing key reservation is declared before its
accumulator so the accumulator is dropped first on error. Reserving this
zero-byte owner now happens before accumulator construction; under the query
tracker invariant the reservation does not check interruption or exhaust the
budget. A synchronized cancellation probe observes an actual later update and
eventual lease release; it does not observe an individual Value destructor.

This is a partial error-path repair. The returned Value still leaves this
separate reservation behind on success, and accumulator updates and merges
still require their own pre-admission and clone/transition ownership work.
Tests remain unexecuted until the consolidated validation gate.

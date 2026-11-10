# Binary INTERVAL framing

PostgreSQL 18.6 reads exactly sixteen bytes for a binary INTERVAL and performs
no component-range validation in `interval_recv`: zero, canonical infinity,
and mixed i64/i32 extrema are accepted. A short frame is 08P01 `insufficient
data left in message`; a valid prefix with extra data is 22P03, with a Bind
parameter ordinal, array element ordinal or COPY-specific message. Raw scalar
Bind and scalar/array COPY probes confirmed these cases; the oracle was stopped.

Retain the public `receive_interval` signature and short-frame text. At the
private codec boundary represent a short interval as a typed 08P01 error and
extend the existing narrow temporal excess/array/COPY transport to INTERVAL.
This changes existing generic error mapping without imposing new content
validation. Interval typmod application remains a separate pending boundary.
Authored probes are unexecuted pending consolidated validation and do not
complete #815.

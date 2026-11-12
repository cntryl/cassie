# Owned row error through a worker and pgwire frame

This test-only #809 experiment follows the selected private execution-error
direction. It does not change a public error enum, enable a storage path, or
route production queries through a new API. A helper reads a real row-blob
field through the isolated checked directory/cell candidate, returns the
admitted row parse diagnostic from a worker thread, and builds a pgwire error
frame with SQLSTATE 42601 while retaining the diagnostic during frame creation.
After construction the frame's own reservation must survive independently.

Probes compare the old row error's bare descriptor message, requested ERROR
and FATAL severity, exact/below frame overlap, actual encoded-frame copy
cancellation and cleanup. The cancellation probe directly exercises the
existing observed frame builder with an admitted message; it does not claim to
observe the earlier `controlled_diagnostic` message-copy phase. The public
embedded conversion to `CassieError::Parse(String)` and its detached String
remain explicitly unqualified; a separate test-only conversion now records
its exact display/descriptor semantics and demonstrates that the query lease
is released while the public String remains alive. This does not count that
public allocation or its subsequent clones. This experiment stops at the owned encoded
frame; it does not prove an actual socket write, REST body, TLS/network buffer
or transport flush. All tests remain unexecuted.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = cassie::midge::adapter::decode_column_chunk_for_test(data);
});

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = cassie::midge::adapter::decode_column_batch_manifest_for_test(data);
});

#![no_main]
use gcmodule::testutil::test_small_graph;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (u8, u16, u16, Vec<u8>)| {
    let (n, atomic_bits, collect_bits, edges) = data;
    test_small_graph(((n as usize) % 16) + 1, &edges, atomic_bits, collect_bits);
});

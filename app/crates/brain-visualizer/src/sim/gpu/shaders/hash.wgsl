// BV22 — WGSL hash, MUST match src/connectivity/hash.rs verbatim.
// Constants and operation order are locked. The native determinism test
// (tests/wgsl_hash_determinism.rs) embeds this exact source and asserts the
// GPU output equals the Rust output for the golden vectors. Do not edit one
// side without the other.

fn hash32(x_in: u32) -> u32 {
    var x = x_in;
    x ^= x >> 16u;
    x = x * 0x7feb352du;
    x ^= x >> 15u;
    x = x * 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}

fn mix_key(seed_lo: u32, neuron_id: u32, synapse_j: u32, salt: u32) -> u32 {
    let k = seed_lo
        ^ (neuron_id * 0x9e3779b1u)
        ^ (synapse_j * 0x85ebca6bu)
        ^ (salt * 0xc2b2ae35u);
    return hash32(k);
}

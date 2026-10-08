// quick check: pools.json deserializes to PoolDescriptor via allbright-core
fn main() {
    let pools = allbright_core::chain::load_pools();
    println!("loaded {} pools", pools.len());
    let by = allbright_core::chain::pools_by_chain(&pools);
    for (c, v) in &by { println!("  {:?}: {}", c, v.len()); }
    assert!(pools.iter().all(|p| p.factory.is_some()));
}

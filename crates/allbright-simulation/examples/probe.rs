//! Probe: connect to the sim bus, send a SimRequest, await a SimResult.
use allbright_core::bus::BusClient;
use allbright_core::events::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr = std::env::args().nth(1).unwrap_or("127.0.0.1:3202".into());
    let cli = BusClient::connect_retry(&addr).await;
    let (mut rx, tx) = cli.split();

    // Real cross-venue pair from pools.json: BSC ETH/WBNB on wault
    // (0x0425…) and sushiswap (0x6da1…) — cyclic route A->B->A.
    let eth = "0x2170ed0880ac9a755fd29b2688956bd959f933f8";
    let wbnb = "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c";
    let wault = "0x04253ab3ff54d2e03b717bf6810a0a2fd228365a";
    let sushi = "0x6da1ffd9d24e1753d2f46ca53146116c7210b3c5";
    let req = SimRequest {
        req_id: 9999,
        chain: Chain::Bsc,
        route: vec![
            SwapLeg { venue: "wault".into(), pool_address: wault.into(),
                      token_in: eth.into(), token_out: wbnb.into(), dex: DexKind::V2 },
            SwapLeg { venue: "sushiswap".into(), pool_address: sushi.into(),
                      token_in: wbnb.into(), token_out: eth.into(), dex: DexKind::V2 },
        ],
        input_amount: 1.0,
        input_token: eth.into(),
        block: 0,
        ts_ms: now_ms(),
    };
    tx.send(&Event::SimRequest(req))?;
    println!("sent SimRequest 9999");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_secs(15), rx.recv()).await {
            Ok(Some(Event::SimResult(r))) => {
                println!("SimResult: ok={} out={} req_id={} detail={}", r.ok, r.output_amount, r.req_id, r.detail);
                return Ok(());
            }
            Ok(Some(_)) => continue,
            _ => { println!("timeout/no result"); return Ok(()); }
        }
    }
    Ok(())
}

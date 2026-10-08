//! Benchmark: route evaluation against the engineering targets
//! (<50 ms warm per-opportunity; <120 s full-cycle worst case per ToR).

use criterion::{Criterion, black_box, criterion_group, criterion_main};

// Inline the math module — benches can't import bin internals.
#[path = "../src/math.rs"]
mod math;

use allbright_core::events::{Chain, DexKind, PoolState, SwapLeg, Token};

fn pool(addr: &str) -> PoolState {
    PoolState {
        chain: Chain::Bsc,
        dex: DexKind::V2,
        venue: "bench".into(),
        address: addr.into(),
        token0: Token { symbol: "USDT".into(), address: "0xa".into(), decimals: 18 },
        token1: Token { symbol: "WBNB".into(), address: "0xb".into(), decimals: 18 },
        reserve0: 4_000_000.0,
        reserve1: 6_000.0,
        sqrt_price_x96: None,
        liquidity: None,
        fee_bps: 25,
        block: 1,
        ts_ms: 0,
    }
}

fn bench_route(c: &mut Criterion) {
    let p1 = pool("0x1");
    let p2 = pool("0x2");
    let legs = vec![
        SwapLeg {
            venue: "a".into(), pool_address: "0x1".into(),
            token_in: "USDT".into(), token_out: "WBNB".into(), dex: DexKind::V2,
        },
        SwapLeg {
            venue: "b".into(), pool_address: "0x2".into(),
            token_in: "WBNB".into(), token_out: "USDT".into(), dex: DexKind::V2,
        },
    ];
    c.bench_function("v2_2hop_route", |b| {
        b.iter(|| {
            math::route_out(
                |addr| if addr == "0x1" { Some(&p1) } else { Some(&p2) },
                black_box(&legs),
                "USDT",
                black_box(10_000.0),
            )
        })
    });
}

criterion_group!(benches, bench_route);
criterion_main!(benches);

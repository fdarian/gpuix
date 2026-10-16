//! What parsing the CSS value strings of one element costs. Run with
//! `CARGO_PROFILE_RELEASE_LTO=off cargo test --release --lib parse_cost -- --ignored --nocapture`;
//! a debug build says nothing about the cost.

use std::hint::black_box;
use std::time::Instant;

use crate::color::{ColorContext, Scheme};
use crate::style::StyleDesc;

const BACKGROUND_IMAGE: &str = "linear-gradient(135deg, #ff7e5f 0%, #feb47b 50%, rgba(10, 20, 30, 0.8) 100%), \
     radial-gradient(circle at 30% 20%, rgba(255, 255, 255, 0.4), transparent 60%), \
     conic-gradient(from 90deg at 50% 50%, red, yellow, lime, aqua, blue, magenta, red)";
const BOX_SHADOW: &str = "0 1px 2px rgba(0, 0, 0, .2), 0 4px 8px rgba(0, 0, 0, .15), inset 0 0 0 1px rgba(255, 255, 255, .1)";
const FILTER: &str = "blur(4px) brightness(1.1) contrast(0.9) drop-shadow(0 2px 4px rgba(0, 0, 0, .3))";
const MASK_IMAGE: &str = "linear-gradient(to bottom, black 80%, transparent)";
const CLIP_PATH: &str = "polygon(50% 0%, 100% 38%, 82% 100%, 18% 100%, 0% 38%)";

fn time(label: &str, mut run: impl FnMut()) {
    const ITERATIONS: u32 = 20_000;
    const ROUNDS: usize = 9;
    for _ in 0..ITERATIONS / 10 {
        run();
    }
    let mut rounds: Vec<f64> = (0..ROUNDS)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..ITERATIONS {
                run();
            }
            start.elapsed().as_nanos() as f64 / ITERATIONS as f64
        })
        .collect();
    rounds.sort_by(f64::total_cmp);
    println!(
        "[parse_cost] {label:<28} min {:>8.0} ns  p50 {:>8.0} ns",
        rounds[0],
        rounds[ROUNDS / 2]
    );
}

#[test]
#[ignore = "a measurement, not a check"]
fn parse_cost() {
    let colors = ColorContext::root(Scheme::Light);
    let style: StyleDesc = serde_json::from_value(serde_json::json!({
        "backgroundImage": BACKGROUND_IMAGE,
        "maskImage": MASK_IMAGE,
    }))
    .unwrap();

    time("background-image (3 layers)", || {
        black_box(style.resolved_fill(black_box(&colors)));
    });
    time("box-shadow (3 layers)", || {
        black_box(super::box_shadow::parse(black_box(BOX_SHADOW)));
    });
    time("filter (4 functions)", || {
        black_box(super::filter::parse_resolved(black_box(FILTER), black_box(&colors)));
    });
    time("mask-image (1 gradient)", || {
        black_box(style.resolved_mask(black_box(&colors)));
    });
    time("clip-path (polygon)", || {
        black_box(super::clip_path::parse(black_box(CLIP_PATH)));
    });
}

/// A string that differs every frame, as a motion interpolating a gradient
/// sends: the cache can only add its lookup and insert to the parse.
#[test]
#[ignore = "a measurement, not a check"]
fn parse_cost_of_a_string_that_never_repeats() {
    let colors = ColorContext::root(Scheme::Light);
    let texts: Vec<String> = (0..4096)
        .map(|index| format!("linear-gradient({index}deg, #ff7e5f 0%, #feb47b 50%, rgba(10, 20, 30, 0.8) 100%)"))
        .collect();
    let mut next = 0;
    time("fresh gradient, cached", || {
        next = (next + 1) % texts.len();
        black_box(super::gradient::parse(black_box(&texts[next]), black_box(&colors)).ok());
    });
    time("fresh gradient, uncached", || {
        next = (next + 1) % texts.len();
        black_box(super::gradient::parse_uncached(black_box(&texts[next]), black_box(&colors)).ok());
    });
}

#![feature(test)]

extern crate test;
#[macro_use]
extern crate phoenix_tls;

use phoenix_tls::NoSubscribe;
use test::Bencher;

struct A;

impl Default for A {
    fn default() -> Self {
        println!("here");
        A
    }
}

phoenix_tls::phoenix_tls! {
    static VAL: NoSubscribe<A>;
}

#[bench]
fn with(b: &mut Bencher) {
    b.iter(|| {
        for _ in 0..1_000_000 {
            drop(VAL.with(|_| {}))
        }
    })
}

#[bench]
fn get(b: &mut Bencher) {
    b.iter(|| {
        for _ in 0..1_000_000 {
            drop(VAL.handle())
        }
    })
}

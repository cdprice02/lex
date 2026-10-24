//! Throwaway microbenchmark: per-guess cost of turning K candidate words into an entropy at
//! word length N, for three ways of counting patterns (dense buckets, sort-and-run, open-address
//! hash) and two ways of computing one pattern (O(N^2) compare, O(N+26) letter counts).
//! Usage: cargo run --release -- [work_scale]
use std::hint::black_box;
use std::time::Instant;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// English-ish letter weights (per 1000) so patterns have realistic green/yellow overlap.
const W: [u32; 26] = [
    82, 15, 28, 43, 127, 22, 20, 61, 70, 2, 8, 40, 24, 67, 75, 19, 1, 60, 63, 91, 28, 10, 24, 2,
    20, 1,
];

fn letter(r: &mut Rng) -> u8 {
    let total: u32 = W.iter().sum();
    let mut x = (r.next() % total as u64) as u32;
    for (i, &w) in W.iter().enumerate() {
        if x < w {
            return i as u8;
        }
        x -= w;
    }
    25
}

fn words<const N: usize>(r: &mut Rng, k: usize) -> Vec<[u8; N]> {
    (0..k).map(|_| std::array::from_fn(|_| letter(r))).collect()
}

/// O(N^2) compare with u32 bitmasks (the current algorithm widened). Returns the base-3 ordinal.
#[inline]
fn ord_quad<const N: usize>(word: &[u8; N], guess: &[u8; N]) -> u64 {
    let mut green = 0u32;
    for i in 0..N {
        if word[i] == guess[i] {
            green |= 1 << i;
        }
    }
    let mut claimed = green;
    let mut ord = 0u64;
    for i in 0..N {
        let d = if green >> i & 1 == 1 {
            2
        } else {
            let mut found = 0;
            for j in 0..N {
                if claimed >> j & 1 == 0 && word[j] == guess[i] {
                    claimed |= 1 << j;
                    found = 1;
                    break;
                }
            }
            found
        };
        ord = ord * 3 + d;
    }
    ord
}

/// O(N + 26) with a per-letter count of unmatched word letters.
#[inline]
fn ord_counts<const N: usize>(word: &[u8; N], guess: &[u8; N]) -> u64 {
    let mut cnt = [0u8; 26];
    for i in 0..N {
        if word[i] != guess[i] {
            cnt[word[i] as usize] += 1;
        }
    }
    let mut ord = 0u64;
    for i in 0..N {
        let d = if word[i] == guess[i] {
            2
        } else if cnt[guess[i] as usize] > 0 {
            cnt[guess[i] as usize] -= 1;
            1
        } else {
            0
        };
        ord = ord * 3 + d;
    }
    ord
}

struct Dense {
    buckets: Vec<f64>,
    touched: Vec<u32>,
}

fn dense<const N: usize>(d: &mut Dense, g: &[u8; N], ws: &[[u8; N]], ps: &[f64]) -> f64 {
    for (w, &p) in ws.iter().zip(ps) {
        let o = ord_quad(w, g) as usize;
        if d.buckets[o] == 0.0 {
            d.touched.push(o as u32);
        }
        d.buckets[o] += p;
    }
    d.touched.sort_unstable();
    let mut acc = 0.0;
    for &o in d.touched.iter() {
        let p = std::mem::take(&mut d.buckets[o as usize]);
        acc += p * p.log2();
    }
    d.touched.clear();
    0.0 - acc
}

fn sort_count<const N: usize>(
    buf: &mut Vec<(u64, f64)>,
    g: &[u8; N],
    ws: &[[u8; N]],
    ps: &[f64],
    counts: bool,
) -> f64 {
    buf.clear();
    for (w, &p) in ws.iter().zip(ps) {
        let o = if counts {
            ord_counts(w, g)
        } else {
            ord_quad(w, g)
        };
        buf.push((o, p));
    }
    buf.sort_unstable_by_key(|x| x.0);
    let mut acc = 0.0;
    let mut i = 0;
    while i < buf.len() {
        let (o, mut p) = buf[i];
        i += 1;
        while i < buf.len() && buf[i].0 == o {
            p += buf[i].1;
            i += 1;
        }
        acc += p * p.log2();
    }
    0.0 - acc
}

/// Open-addressing table, power-of-two capacity >= 2K, cleared per call; distinct entries are
/// sorted by key afterwards so the sum is word-order independent.
fn hash_count<const N: usize>(
    tab: &mut Vec<(u64, f64)>,
    out: &mut Vec<(u64, f64)>,
    g: &[u8; N],
    ws: &[[u8; N]],
    ps: &[f64],
) -> f64 {
    let cap = (2 * ws.len()).next_power_of_two().max(16);
    tab.clear();
    tab.resize(cap, (u64::MAX, 0.0));
    let mask = cap - 1;
    for (w, &p) in ws.iter().zip(ps) {
        let o = ord_quad(w, g);
        let mut h = (o.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 20) as usize & mask;
        loop {
            let e = &mut tab[h];
            if e.0 == o {
                e.1 += p;
                break;
            }
            if e.0 == u64::MAX {
                *e = (o, p);
                break;
            }
            h = (h + 1) & mask;
        }
    }
    out.clear();
    out.extend(tab.iter().copied().filter(|e| e.0 != u64::MAX));
    out.sort_unstable_by_key(|x| x.0);
    let mut acc = 0.0;
    for e in out.iter() {
        acc += e.1 * e.1.log2();
    }
    0.0 - acc
}

/// Runtime-width variant of `ord_counts`: words are flat slices of `n` bytes.
#[inline(never)]
fn ord_counts_dyn(word: &[u8], guess: &[u8]) -> u64 {
    let mut cnt = [0u8; 26];
    for i in 0..word.len() {
        if word[i] != guess[i] {
            cnt[word[i] as usize] += 1;
        }
    }
    let mut ord = 0u64;
    for i in 0..word.len() {
        let d = if word[i] == guess[i] {
            2
        } else if cnt[guess[i] as usize] > 0 {
            cnt[guess[i] as usize] -= 1;
            1
        } else {
            0
        };
        ord = ord * 3 + d;
    }
    ord
}

fn dyn_vs_const<const N: usize>(k: usize) {
    let mut r = Rng(7 + N as u64);
    let ws = words::<N>(&mut r, k);
    let flat: Vec<u8> = ws.iter().flatten().copied().collect();
    let g = words::<N>(&mut r, 1)[0];
    let reps = (100_000 / k).max(3);
    let tc = time(reps, || ws.iter().map(|w| ord_counts(w, &g) as f64).sum::<f64>());
    let td = time(reps, || {
        flat.chunks_exact(N)
            .map(|w| ord_counts_dyn(w, &g) as f64)
            .sum::<f64>()
    });
    println!("dyn_vs_const N={N:2} K={k}: const {tc:8.1} us, runtime-width {td:8.1} us");
}

/// Microseconds per call: the minimum over 9 trials of `reps` calls, so descheduling by other
/// processes on a shared machine inflates a trial but not the reported figure.
fn time<F: FnMut() -> f64>(reps: usize, mut f: F) -> f64 {
    black_box(f());
    let mut best = f64::MAX;
    for _ in 0..9 {
        let t = Instant::now();
        let mut s = 0.0;
        for _ in 0..reps {
            s += f();
        }
        black_box(s);
        best = best.min(t.elapsed().as_secs_f64() / reps as f64 * 1e6);
    }
    best
}

fn run<const N: usize>(ks: &[usize], scale: usize, dense_ok: bool) {
    let mut r = Rng(0x2545_f491_4f6c_dd1d ^ N as u64);
    let mut d = Dense {
        buckets: if dense_ok {
            vec![0.0; 3usize.pow(N as u32)]
        } else {
            vec![]
        },
        touched: vec![],
    };
    let (mut b1, mut b2, mut b3) = (vec![], vec![], vec![]);
    for &k in ks {
        let ws = words::<N>(&mut r, k);
        let ps: Vec<f64> = vec![1.0 / k as f64; k];
        let guesses = words::<N>(&mut r, 8);
        let reps = (scale / k).max(3);
        let mut gi = 0usize;
        let mut next = || {
            gi += 1;
            guesses[gi % 8]
        };
        let g = guesses[0];
        let e_s = sort_count(&mut b1, &g, &ws, &ps, false);
        let e_h = hash_count(&mut b2, &mut b3, &g, &ws, &ps);
        let e_c = sort_count(&mut b1, &g, &ws, &ps, true);
        assert_eq!(e_s.to_bits(), e_h.to_bits());
        assert_eq!(e_s.to_bits(), e_c.to_bits());
        let t_pq = time(reps, || {
            let g = next();
            ws.iter().map(|w| ord_quad(w, &g) as f64).sum::<f64>()
        });
        let t_pc = time(reps, || {
            let g = next();
            ws.iter().map(|w| ord_counts(w, &g) as f64).sum::<f64>()
        });
        let t_sort = time(reps, || {
            let g = next();
            sort_count(&mut b1, &g, &ws, &ps, false)
        });
        let t_sortc = time(reps, || {
            let g = next();
            sort_count(&mut b1, &g, &ws, &ps, true)
        });
        let t_hash = time(reps, || {
            let g = next();
            hash_count(&mut b2, &mut b3, &g, &ws, &ps)
        });
        let t_dense = if dense_ok {
            let e_d = dense(&mut d, &g, &ws, &ps);
            assert_eq!(e_s.to_bits(), e_d.to_bits());
            time(reps, || {
                let g = next();
                dense(&mut d, &g, &ws, &ps)
            })
        } else {
            f64::NAN
        };
        println!(
            "N={N:2} K={k:6} | pat_quad {t_pq:9.1} pat_cnt {t_pc:9.1} | dense {t_dense:9.1} sort {t_sort:9.1} hash {t_hash:9.1} sort+cnt {t_sortc:9.1} (us/guess)"
        );
    }
}

fn main() {
    let scale: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(200_000);
    for n_k in [1000usize, 10_000] {
        dyn_vs_const::<5>(n_k);
        dyn_vs_const::<10>(n_k);
        dyn_vs_const::<16>(n_k);
        dyn_vs_const::<20>(n_k);
    }
    let ks = [10usize, 100, 1000, 10_000, 50_000];
    run::<5>(&ks, scale, true);
    run::<8>(&ks, scale, true);
    run::<10>(&ks, scale, true);
    run::<12>(&ks, scale, true);
    run::<14>(&ks, scale, true);
    run::<16>(&ks, scale, false);
    run::<20>(&ks, scale, false);
    run::<24>(&ks, scale, false);
}

//! Throwaway spike for the techniques ticket: evaluates one-ply scores, rollout, and endgame
//! settings by walking the whole policy tree. Not for merging.
#![allow(clippy::needless_range_loop, clippy::unnecessary_sort_by)]

use std::collections::HashMap;
use std::time::Instant;

use anyhow::{Context, bail};
use lex_data::{DataDir, Language, Word};

const PATTERNS: usize = 243;
const WIN: u8 = 242;
const REPORT_T: f64 = 5.0;

type W5 = [u8; 5];

fn pattern(g: &W5, a: &W5) -> u8 {
    let mut green = 0u8;
    for i in 0..5 {
        if g[i] == a[i] {
            green |= 1 << i;
        }
    }
    let mut claimed = green;
    let mut ord = 0u8;
    for i in 0..5 {
        let d = if green >> i & 1 == 1 {
            2
        } else {
            let mut d = 0;
            for j in 0..5 {
                if claimed >> j & 1 == 0 && a[j] == g[i] {
                    claimed |= 1 << j;
                    d = 1;
                    break;
                }
            }
            d
        };
        ord = ord * 3 + d;
    }
    ord
}

struct World {
    names: Vec<String>,
    words: Vec<W5>,
    ln_f: Vec<f64>,
    is_answer: Vec<bool>,
    answers: Vec<u32>,
    /// answer position for each guess index, or u32::MAX
    pos: Vec<u32>,
    /// guess-major, answer positions as columns
    matrix: Option<Vec<u8>>,
    /// report weights per guess index (zero for non-answers), summing to 1 over the answers
    report_w: Vec<f64>,
}

impl World {
    #[inline]
    fn pat(&self, g: u32, a: u32) -> u8 {
        match &self.matrix {
            Some(m) => m[g as usize * self.answers.len() + self.pos[a as usize] as usize],
            None => pattern(&self.words[g as usize], &self.words[a as usize]),
        }
    }

    /// lex's `Pool::prior`: softmax of ln f over T relative to the subset's maximum
    fn prior(&self, set: &[u32], t: f64) -> Vec<f64> {
        let max = set
            .iter()
            .map(|&i| self.ln_f[i as usize])
            .fold(f64::NEG_INFINITY, f64::max);
        let e: Vec<f64> = set
            .iter()
            .map(|&i| ((self.ln_f[i as usize] - max) / t).exp())
            .collect();
        let s: f64 = e.iter().sum();
        e.into_iter().map(|x| x / s).collect()
    }

    fn load(track: &str) -> anyhow::Result<Self> {
        let ws = DataDir::new("data").load::<5>(Language::English, None)?;
        let mut letters: HashMap<char, u8> = HashMap::new();
        let mut enc = |w: &Word<5>| -> W5 {
            std::array::from_fn(|i| {
                let n = letters.len() as u8;
                *letters.entry(w[i]).or_insert(n)
            })
        };
        let corpus: Vec<(String, W5, u64)> = ws
            .word_frequencies()
            .map(|(w, f)| (w.to_string(), enc(&w), f))
            .collect();
        let (names, words, freqs, is_answer): (Vec<String>, Vec<W5>, Vec<u64>, Vec<bool>) =
            match track {
                "eng5" => {
                    let n = corpus.len();
                    let (a, b, c): (Vec<_>, Vec<_>, Vec<_>) = corpus.into_iter().fold(
                        (vec![], vec![], vec![]),
                        |(mut a, mut b, mut c), (s, w, f)| {
                            a.push(s);
                            b.push(w);
                            c.push(f);
                            (a, b, c)
                        },
                    );
                    (a, b, c, vec![true; n])
                }
                "wordle" => {
                    let read = |p: &str| -> anyhow::Result<Vec<String>> {
                        Ok(std::fs::read_to_string(p)
                            .with_context(|| p.to_string())?
                            .split_whitespace()
                            .map(str::to_string)
                            .collect())
                    };
                    let answers = read("data/profiles/answers.txt")?;
                    let guesses = read("data/profiles/guesses.txt")?;
                    if answers.len() != 2315 || guesses.len() != 10657 {
                        bail!("unexpected list sizes");
                    }
                    let freq: HashMap<&str, u64> =
                        corpus.iter().map(|(s, _, f)| (s.as_str(), *f)).collect();
                    let min = corpus.iter().map(|c| c.2).min().unwrap();
                    let mut all: Vec<(String, bool)> = answers
                        .iter()
                        .map(|s| (s.clone(), true))
                        .chain(guesses.iter().map(|s| (s.clone(), false)))
                        .collect();
                    all.sort();
                    // frequency-descending, alphabetical on ties; unseen words take the minimum
                    let mut rows: Vec<(String, u64, bool)> = all
                        .into_iter()
                        .map(|(s, a)| {
                            let f = freq.get(s.as_str()).copied().unwrap_or(min);
                            (s, f, a)
                        })
                        .collect();
                    rows.sort_by(|x, y| y.1.cmp(&x.1));
                    let mut out = (vec![], vec![], vec![], vec![]);
                    for (s, f, a) in rows {
                        let w = Word::<5>::try_from(s.as_str()).unwrap();
                        out.1.push(enc(&w));
                        out.0.push(s);
                        out.2.push(f);
                        out.3.push(a);
                    }
                    out
                }
                _ => bail!("track is wordle or eng5"),
            };
        let ln_f: Vec<f64> = freqs.iter().map(|&f| (f as f64).ln()).collect();
        let answers: Vec<u32> = (0..words.len() as u32)
            .filter(|&i| is_answer[i as usize])
            .collect();
        let mut pos = vec![u32::MAX; words.len()];
        for (p, &a) in answers.iter().enumerate() {
            pos[a as usize] = p as u32;
        }
        let matrix = (words.len() * answers.len() <= 100_000_000).then(|| {
            let mut m = Vec::with_capacity(words.len() * answers.len());
            for g in &words {
                for &a in &answers {
                    m.push(pattern(g, &words[a as usize]));
                }
            }
            m
        });
        let mut report_w = vec![0.0; words.len()];
        let rp = if track == "wordle" {
            vec![1.0 / answers.len() as f64; answers.len()]
        } else {
            let mut w = World {
                names: vec![],
                words: vec![],
                ln_f: ln_f.clone(),
                is_answer: vec![],
                answers: vec![],
                pos: vec![],
                matrix: None,
                report_w: vec![],
            };
            w.ln_f = ln_f.clone();
            w.prior(&answers, REPORT_T)
        };
        for (k, &a) in answers.iter().enumerate() {
            report_w[a as usize] = rp[k];
        }
        Ok(World {
            names,
            words,
            ln_f,
            is_answer,
            answers,
            pos,
            matrix,
            report_w,
        })
    }
}

#[derive(Clone, Copy, Debug)]
enum Score {
    Entropy,
    /// (1 - beta) H + beta log2(bins)
    Blend(f64),
    /// bins first, entropy as the tie-break
    BinsLex,
    /// -log2(sum p_b^2)
    Renyi2,
    /// minimize 1 + sum over non-win buckets of p_b F(H_b)
    Value,
}

#[derive(Clone, Debug)]
struct Config {
    temp: f64,
    win: f64,
    score: Score,
    endgame: usize,
    rollout: Option<(usize, usize, usize)>,
}

/// mass-weighted mean remaining cost per bucket of set entropy, linearly interpolated
#[derive(Clone, Debug)]
struct ValueFn {
    width: f64,
    table: Vec<f64>,
}

impl ValueFn {
    fn eval(&self, h: f64) -> f64 {
        if h <= 0.0 {
            return 1.0;
        }
        let x = h / self.width;
        let i = x.floor() as usize;
        let n = self.table.len();
        if i + 1 >= n {
            let slope = self.table[n - 1] - self.table[n - 2];
            return self.table[n - 1] + slope * (x - (n - 1) as f64);
        }
        let f = x - i as f64;
        self.table[i] * (1.0 - f) + self.table[i + 1] * f
    }
}

struct Policy<'w> {
    w: &'w World,
    cfg: Config,
    vf: Option<ValueFn>,
    weights: Vec<f64>,
    endgame_memo: HashMap<Vec<u32>, (f64, u32)>,
    base_memo: HashMap<Vec<u32>, f64>,
    /// (set entropy bits, expected guesses, mass) per node seen by the base cost recursion
    samples: Vec<(f64, f64, f64)>,
    record: bool,
    /// distinct labelled partitions / guesses scanned, per depth bucket
    dedupe: Option<HashMap<usize, (u64, u64, u64)>>,
}

struct Bucket {
    touched: Vec<u8>,
    mass: [f64; PATTERNS],
    plp: [f64; PATTERNS],
}

impl<'w> Policy<'w> {
    fn new(w: &'w World, cfg: Config, vf: Option<ValueFn>) -> Self {
        let max = w
            .answers
            .iter()
            .map(|&i| w.ln_f[i as usize])
            .fold(f64::NEG_INFINITY, f64::max);
        let weights = w
            .ln_f
            .iter()
            .map(|&l| ((l - max) / cfg.temp).exp().max(f64::MIN_POSITIVE))
            .collect();
        Self {
            w,
            cfg,
            vf,
            weights,
            endgame_memo: HashMap::new(),
            base_memo: HashMap::new(),
            samples: vec![],
            record: false,
            dedupe: None,
        }
    }

    /// every guess scored, in pool order: (primary, secondary, in_candidates, guess)
    fn scored(&mut self, set: &[u32], depth: usize) -> Vec<(f64, f64, bool, u32)> {
        let w = self.w;
        let probs = w.prior(set, self.cfg.temp);
        let plp: Vec<f64> = probs.iter().map(|&p| p * p.log2()).collect();
        let mut b = Bucket {
            touched: Vec::with_capacity(PATTERNS),
            mass: [0.0; PATTERNS],
            plp: [0.0; PATTERNS],
        };
        let mut out = Vec::with_capacity(w.words.len());
        let mut next = 0;
        let mut seen: Option<HashMap<Vec<u8>, ()>> = self.dedupe.as_ref().map(|_| HashMap::new());
        let mut splitless = 0u64;
        let mut labels: Vec<u8> = Vec::with_capacity(set.len());
        for g in 0..w.words.len() as u32 {
            let (p_ans, in_c) = match set.get(next) {
                Some(&c) if c == g => {
                    next += 1;
                    (probs[next - 1], true)
                }
                _ => (0.0, false),
            };
            labels.clear();
            for (k, &a) in set.iter().enumerate() {
                let o = w.pat(g, a);
                if seen.is_some() {
                    labels.push(o);
                }
                let oi = o as usize;
                if b.mass[oi] == 0.0 {
                    b.touched.push(o);
                }
                b.mass[oi] += probs[k];
                b.plp[oi] += plp[k];
            }
            if let Some(s) = seen.as_mut() {
                if !in_c && b.touched.len() == 1 {
                    splitless += 1;
                }
                s.insert(labels.clone(), ());
            }
            b.touched.sort_unstable();
            let mut acc = 0.0f64;
            let mut s2 = 0.0;
            let mut ev = 1.0;
            let bins = b.touched.len() as f64;
            for &o in &b.touched {
                let oi = o as usize;
                let m = std::mem::take(&mut b.mass[oi]);
                let pl = std::mem::take(&mut b.plp[oi]);
                acc += m * m.log2();
                s2 += m * m;
                if let (Some(vf), true) = (&self.vf, o != WIN) {
                    // entropy of the bucket renormalized: -(sum p log p)/m + log m
                    let hb = (-pl / m + m.log2()).max(0.0);
                    ev += m * vf.eval(hb);
                }
            }
            b.touched.clear();
            let h = 0.0 - acc;
            let bonus = self.cfg.win * p_ans;
            let (p1, p2) = match self.cfg.score {
                Score::Entropy => (h + bonus, 0.0),
                Score::Blend(beta) => ((1.0 - beta) * h + beta * bins.log2() + bonus, 0.0),
                Score::BinsLex => (bins, h + bonus),
                Score::Renyi2 => (-(s2.log2()) + bonus, 0.0),
                Score::Value => (-ev, 0.0),
            };
            out.push((p1, p2, in_c, g));
        }
        if let (Some(s), Some(d)) = (seen, self.dedupe.as_mut()) {
            let e = d.entry(depth.min(4)).or_default();
            e.0 += s.len() as u64;
            e.1 += w.words.len() as u64;
            e.2 += splitless;
        }
        out
    }

    fn one_ply(&mut self, set: &[u32], depth: usize) -> u32 {
        let mut best: Option<(f64, f64, bool, u32)> = None;
        for c in self.scored(set, depth) {
            best = match best {
                None => Some(c),
                Some(b) => {
                    let better =
                        c.0 > b.0 || (c.0 == b.0 && (c.1 > b.1 || (c.1 == b.1 && c.2 && !b.2)));
                    Some(if better { c } else { b })
                }
            };
        }
        best.unwrap().3
    }

    fn top_k(&mut self, set: &[u32], depth: usize, k: usize) -> Vec<u32> {
        let mut s = self.scored(set, depth);
        s.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap()
                .then(b.1.partial_cmp(&a.1).unwrap())
                .then(b.2.cmp(&a.2))
        });
        s.into_iter().take(k).map(|c| c.3).collect()
    }

    fn choose(&mut self, set: &[u32], depth: usize) -> u32 {
        if set.len() == 1 {
            return set[0];
        }
        if self.cfg.endgame > 0 && set.len() <= self.cfg.endgame {
            return self.endgame(set).1;
        }
        if let Some((k, lo, hi)) = self.cfg.rollout
            && (lo..=hi).contains(&set.len())
        {
            return self.rollout(set, depth, k);
        }
        self.one_ply(set, depth)
    }

    /// children of `set` under `g`, win bucket excluded, each ascending
    fn partition(&self, set: &[u32], g: u32) -> Vec<Vec<u32>> {
        let mut keyed: Vec<(u8, u32)> = set.iter().map(|&a| (self.w.pat(g, a), a)).collect();
        keyed.sort_by_key(|k| k.0);
        keyed
            .chunk_by(|a, b| a.0 == b.0)
            .filter(|grp| grp[0].0 != WIN)
            .map(|grp| grp.iter().map(|k| k.1).collect())
            .collect()
    }

    fn mass(&self, set: &[u32]) -> f64 {
        set.iter().map(|&i| self.weights[i as usize]).sum()
    }

    /// expected guesses from `set` under the policy without rollout
    fn base_cost(&mut self, set: &[u32], depth: usize) -> f64 {
        if set.len() == 1 {
            return 1.0;
        }
        if let Some(&c) = self.base_memo.get(set) {
            return c;
        }
        let g = if self.cfg.endgame > 0 && set.len() <= self.cfg.endgame {
            self.endgame(set).1
        } else {
            self.one_ply(set, depth)
        };
        let mass = self.mass(set);
        let mut cost = 1.0;
        let children = self.partition(set, g);
        assert!(
            !(children.len() == 1 && children[0].len() == set.len()),
            "guess does not split"
        );
        for child in children {
            cost += self.mass(&child) / mass * self.base_cost(&child, depth + 1);
        }
        if self.record {
            let p = self.w.prior(set, self.cfg.temp);
            let h: f64 = 0.0 - p.iter().map(|&x| x * x.log2()).sum::<f64>();
            self.samples.push((h, cost, mass));
        }
        self.base_memo.insert(set.to_vec(), cost);
        cost
    }

    fn rollout(&mut self, set: &[u32], depth: usize, k: usize) -> u32 {
        let mass = self.mass(set);
        let mut best: Option<(f64, u32)> = None;
        for g in self.top_k(set, depth, k) {
            let mut cost = 1.0;
            for child in self.partition(set, g) {
                if child.len() == set.len() {
                    cost = f64::INFINITY;
                    break;
                }
                cost += self.mass(&child) / mass * self.base_cost(&child, depth + 1);
            }
            if best.is_none_or(|b| cost < b.0 - 1e-12) {
                best = Some((cost, g));
            }
        }
        best.unwrap().1
    }

    /// port of lex's Endgame (plain expectation): (weighted cost, guess)
    fn endgame(&mut self, set: &[u32]) -> (f64, u32) {
        if let [only] = *set {
            return (self.weights[only as usize], only);
        }
        if let Some(&s) = self.endgame_memo.get(set) {
            return s;
        }
        let w = self.w;
        let mass = self.mass(set);
        let mut order: Vec<(f64, bool, u32)> = Vec::new();
        let mut keyed: Vec<(u8, u32)> = Vec::with_capacity(set.len());
        let mut consider = |this: &Self, g: u32, in_set: bool| {
            keyed.clear();
            keyed.extend(set.iter().map(|&a| (w.pat(g, a), a)));
            keyed.sort_by_key(|k| k.0);
            if !in_set && keyed.first().map(|k| k.0) == keyed.last().map(|k| k.0) {
                return;
            }
            let mut sum = mass;
            for grp in keyed.chunk_by(|a, b| a.0 == b.0) {
                if grp[0].0 == WIN {
                    continue;
                }
                let (m, top) = grp.iter().fold((0.0, 0.0f64), |(m, t), &(_, i)| {
                    let x = this.weights[i as usize];
                    (m + x, t.max(x))
                });
                sum += 2.0 * m - top;
            }
            order.push((sum, !in_set, g));
        };
        for &g in set {
            consider(self, g, true);
        }
        let mut next = 0;
        for g in 0..w.words.len() as u32 {
            if set.get(next) == Some(&g) {
                next += 1;
            } else {
                consider(self, g, false);
            }
        }
        order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let mut best: Option<(f64, u32)> = None;
        for (floor, outside, g) in order {
            if best.is_some_and(|b| floor > b.0 * (1.0 + 1e-9)) {
                break;
            }
            let mut cost = mass;
            let mut pruned = false;
            for child in self.partition(set, g) {
                cost += self.endgame(&child).0;
                if best.is_some_and(|b| cost > b.0) {
                    pruned = true;
                    break;
                }
            }
            if pruned {
                continue;
            }
            let rank = |g: u32| (set.binary_search(&g).is_err(), g);
            let ahead = |b: (f64, u32)| cost < b.0 || (cost == b.0 && rank(g) < rank(b.1));
            let _ = outside;
            if best.is_none_or(ahead) {
                best = Some((cost, g));
            }
        }
        let s = best.unwrap();
        self.endgame_memo.insert(set.to_vec(), s);
        s
    }

    /// plays every target in `targets` (a subset of `set`), writing guesses to `out`
    fn eval(&mut self, set: &[u32], targets: &[u32], depth: usize, out: &mut [u8]) {
        let g = self.choose(set, depth);
        if depth == 0 {
            eprintln!("opener {}", self.w.names[g as usize]);
        }
        let mut keyed: Vec<(u8, u32)> = set.iter().map(|&a| (self.w.pat(g, a), a)).collect();
        keyed.sort_by_key(|k| k.0);
        let mut tk: Vec<(u8, u32)> = targets.iter().map(|&a| (self.w.pat(g, a), a)).collect();
        tk.sort_by_key(|k| k.0);
        for tg in tk.chunk_by(|a, b| a.0 == b.0) {
            let o = tg[0].0;
            if o == WIN {
                out[tg[0].1 as usize] = depth as u8 + 1;
                continue;
            }
            let child: Vec<u32> = keyed.iter().filter(|k| k.0 == o).map(|k| k.1).collect();
            let ct: Vec<u32> = tg.iter().map(|k| k.1).collect();
            assert!(child.len() < set.len() || depth < 30, "no progress");
            self.eval(&child, &ct, depth + 1, out);
        }
    }
}

/// fits the value function from the plain policy's own tree on this track
fn fit(w: &World, cfg: &Config) -> ValueFn {
    let base = Config {
        score: Score::Entropy,
        rollout: None,
        endgame: 0,
        ..cfg.clone()
    };
    let mut p = Policy::new(w, base, None);
    p.record = true;
    p.base_cost(&w.answers, 0);
    let width = 0.25;
    let n = (p.samples.iter().map(|s| s.0).fold(0.0, f64::max) / width).ceil() as usize + 2;
    let mut acc = vec![(0.0, 0.0); n];
    for &(h, c, m) in &p.samples {
        let x = h / width;
        let i = x.floor() as usize;
        let f = x - i as f64;
        acc[i].0 += m * (1.0 - f) * c;
        acc[i].1 += m * (1.0 - f);
        acc[i + 1].0 += m * f * c;
        acc[i + 1].1 += m * f;
    }
    let mut table = vec![1.0; n];
    let mut last = 1.0;
    for i in 1..n {
        table[i] = if acc[i].1 > 0.0 {
            acc[i].0 / acc[i].1
        } else {
            last
        };
        last = table[i];
    }
    eprintln!(
        "value fn from {} nodes: {:?}",
        p.samples.len(),
        table
            .iter()
            .map(|x| (x * 100.0).round() / 100.0)
            .collect::<Vec<_>>()
    );
    ValueFn { width, table }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let track = args
        .first()
        .context("usage: spike <wordle|eng5> [opts]")?
        .clone();
    let mut cfg = Config {
        temp: 5.0,
        win: 1.0,
        score: Score::Entropy,
        endgame: 0,
        rollout: None,
    };
    let mut sample = 0usize;
    let mut out_path: Option<String> = None;
    let mut dedupe = false;
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        let mut v = || it.next().context("missing value").cloned();
        match a.as_str() {
            "--temp" => cfg.temp = v()?.parse()?,
            "--win" => cfg.win = v()?.parse()?,
            "--endgame" => cfg.endgame = v()?.parse()?,
            "--sample" => sample = v()?.parse()?,
            "--out" => out_path = Some(v()?),
            "--dedupe" => dedupe = true,
            "--rollout" => {
                let s = v()?;
                let p: Vec<usize> = s.split(':').map(|x| x.parse().unwrap()).collect();
                cfg.rollout = Some((p[0], p[1], p[2]));
            }
            "--score" => {
                let s = v()?;
                cfg.score = match s.as_str() {
                    "entropy" => Score::Entropy,
                    "bins" => Score::BinsLex,
                    "renyi" => Score::Renyi2,
                    "value" => Score::Value,
                    b if b.starts_with("blend:") => Score::Blend(b[6..].parse()?),
                    _ => bail!("unknown score {s}"),
                };
            }
            "--check" => {
                // pattern() must agree with lex's ordinal
                let w = World::load(&track)?;
                for g in (0..w.words.len()).step_by(37) {
                    for a in (0..w.words.len()).step_by(41) {
                        let gw = Word::<5>::try_from(w.names[g].as_str()).unwrap();
                        let aw = Word::<5>::try_from(w.names[a].as_str()).unwrap();
                        let lex = lex_core::WordCorrectness::correct(aw, gw).ordinal();
                        assert_eq!(lex, pattern(&w.words[g], &w.words[a]) as usize);
                    }
                }
                eprintln!("pattern agrees with lex");
                return Ok(());
            }
            _ => bail!("unknown arg {a}"),
        }
    }

    let w = World::load(&track)?;
    let t0 = Instant::now();
    let vf = matches!(cfg.score, Score::Value).then(|| fit(&w, &cfg));
    let fit_s = t0.elapsed().as_secs_f64();
    let t1 = Instant::now();
    let mut policy = Policy::new(&w, cfg.clone(), vf);
    if dedupe {
        policy.dedupe = Some(HashMap::new());
    }
    let targets: Vec<u32> = if sample > 0 {
        // even stride over frequency order: deterministic, spans common to rare
        let n = w.answers.len();
        (0..sample).map(|i| w.answers[i * n / sample]).collect()
    } else {
        w.answers.clone()
    };
    let mut guesses = vec![0u8; w.words.len()];
    policy.eval(&w.answers, &targets, 0, &mut guesses);
    let secs = t1.elapsed().as_secs_f64();

    let n = targets.len() as f64;
    let g = |t: u32| guesses[t as usize] as f64;
    let uniform = targets.iter().map(|&t| g(t)).sum::<f64>() / n;
    let rw: f64 = targets.iter().map(|&t| w.report_w[t as usize]).sum();
    let weighted = targets
        .iter()
        .map(|&t| w.report_w[t as usize] * g(t))
        .sum::<f64>()
        / rw;
    let fails = targets.iter().filter(|&&t| g(t) > 6.0).count() as f64 / n;
    let max = targets.iter().map(|&t| guesses[t as usize]).max().unwrap();
    // frequency-weighted mean at the canonical temperature, also on the Wordle track
    let freq_w = {
        let p = w.prior(&targets, REPORT_T);
        targets.iter().zip(&p).map(|(&t, &q)| q * g(t)).sum::<f64>()
    };
    let _ = &w.is_answer;
    println!(
        "{track}\t{:?}\ttargets={}\tuniform={uniform:.4}\treport={weighted:.4}\tfreqT5={freq_w:.4}\tfail={:.2}%\tmax={max}\tsecs={secs:.1}\tfit_secs={fit_s:.1}",
        cfg,
        targets.len(),
        fails * 100.0
    );
    if let Some(d) = &policy.dedupe {
        let mut keys: Vec<_> = d.keys().copied().collect();
        keys.sort();
        for k in keys {
            let (distinct, total, splitless) = d[&k];
            println!(
                "dedupe depth {k}{}: distinct partitions {:.1}% of guesses scanned, split-less non-candidates {:.1}%",
                if k == 4 { "+" } else { "" },
                100.0 * distinct as f64 / total as f64,
                100.0 * splitless as f64 / total as f64
            );
        }
    }
    if let Some(p) = out_path {
        let mut s = String::new();
        for &t in &targets {
            s += &format!("{}\t{}\n", w.names[t as usize], guesses[t as usize]);
        }
        std::fs::write(p, s)?;
    }
    Ok(())
}

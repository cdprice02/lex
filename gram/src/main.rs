use std::io::{BufRead, Write};

static DAT_FILE_PATH: &str = "../data/words.dat";
static OUT_FILE_PATH: &str = "../data/words_with_frequencies.csv";

fn main() {
    let input = std::fs::File::open(DAT_FILE_PATH).expect("file valid");
    let input = std::io::BufReader::new(input);
    let output = std::fs::File::create(OUT_FILE_PATH).expect("file valid");
    let mut output = std::io::BufWriter::new(output);

    for line in input.lines() {
        let line = line.expect("failed to read line");
        if line.starts_with('*') {
            continue;
        }

        assert!(
            line.len() >= 5,
            "all lines at least have a five letter word"
        );
        let word = &line[0..5];
        let freq = if line.len() == 5 {
            0
        } else {
            // skip the commonness delimiter at index 5
            let rest = if line.len() >= 6 { &line[6..] } else { "" };

            if rest.is_empty() {
                0
            } else {
                rest.split(',')
                    .filter(|p| !p.is_empty())
                    .map(|p| p.parse::<u64>().expect("frequency is a number"))
                    .sum()
            }
        };

        let output_line = format!("{},{}\n", word, freq);
        output.write_all(output_line.as_bytes()).unwrap();
    }
}

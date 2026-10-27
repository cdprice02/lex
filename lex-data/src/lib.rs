#![cfg_attr(test, feature(test))]
#![feature(non_exhaustive_omitted_patterns_lint)]
#![feature(must_not_suspend)]
#![warn(non_exhaustive_omitted_patterns)]
#![warn(must_not_suspend)]

pub mod language;
pub mod word;

mod data_dir;
mod error;
mod header;
mod http;
mod list;
mod ngrams;
mod parse;
mod profile;
mod profile_store;
mod retry;
mod store;
mod wiktionary;

pub use data_dir::DataDir;
pub use error::LexDataError;
pub use language::Language;
pub use ngrams::SNAPSHOT as NGRAMS_SNAPSHOT;
pub use profile::{ListSpec, Profile, ProfileData, ProfileSpec, ProfileStats, Source};
pub use wiktionary::DictMetadata;
pub use word::{Word, WordSet};

pub const MIN_WORD_LENGTH: usize = 3;
pub const MAX_WORD_LENGTH: usize = 10;

/// The most letters a Corpus Alphabet may hold: the solver encodes a letter as one byte and
/// reserves the last value for a letter outside the Alphabet.
pub const MAX_ALPHABET_SIZE: usize = 255;

#[doc(hidden)]
pub use seq_macro;

/// Dispatches `$f::<N>($args)` over the supported word lengths
/// (`MIN_WORD_LENGTH..=MAX_WORD_LENGTH`), monomorphizing `$f` for each `N`.
///
/// `$f` must be an in-scope function generic over `const N: usize`; `$len` is
/// the runtime word length; `$args` is passed through unchanged. Lengths
/// outside the supported range panic — validate at the CLI boundary first.
#[macro_export]
macro_rules! match_word_length {
    ($f:ident, $len:expr, $args:expr) => {
        $crate::seq_macro::seq!(N in 3..=10 {
            match $len {
                #(N => $f::<N>($args),)*
                _ => ::std::unreachable!("word length outside supported range"),
            }
        })
    };
}

// seq_macro requires literal bounds; this assert keeps the macro's 3..=10 in
// lockstep with the consts above.
const _: () = assert!(
    MIN_WORD_LENGTH == 3 && MAX_WORD_LENGTH == 10,
    "match_word_length! seq bounds must match MIN_WORD_LENGTH / MAX_WORD_LENGTH"
);

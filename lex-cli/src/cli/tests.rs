use super::*;
use crate::config::Targets;
use crate::tracks::Track;
use lex_core::{GuessPool, PriorShape};

mod profile_tests {
    use super::*;

    fn common(args: &[&str]) -> Result<CommonArgs, clap::Error> {
        let Command::Assist(a) =
            Cli::try_parse_from(["lex", "assist"].into_iter().chain(args.iter().copied()))?.command
        else {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::InvalidSubcommand,
                "`assist` parsed to another command",
            ));
        };
        Ok(a.common)
    }

    #[test]
    fn display_flags_are_global_and_default_to_auto_letters() {
        let base = ["lex", "assist", "--lang", "english", "-l", "5"];
        let cli = Cli::try_parse_from(base).unwrap();
        assert_eq!(
            (cli.color, cli.symbols),
            (ColorArg::Auto, SymbolScheme::Letters)
        );
        let cli = Cli::try_parse_from(base.into_iter().chain([
            "--color",
            "always",
            "--symbols",
            "emoji",
        ]))
        .unwrap();
        assert_eq!(
            (cli.color, cli.symbols),
            (ColorArg::Always, SymbolScheme::Emoji)
        );
        let cli = Cli::try_parse_from([
            "lex",
            "--color",
            "never",
            "--symbols",
            "digits",
            "book",
            "clear",
            "--all",
        ])
        .unwrap();
        assert_eq!(
            (cli.color, cli.symbols),
            (ColorArg::Never, SymbolScheme::Digits)
        );
        assert!(Cli::try_parse_from(["lex", "--symbols", "c", "book", "clear", "--all"]).is_err());
    }

    #[test]
    fn language_and_length_are_required_without_a_profile() {
        assert!(common(&[]).is_err());
        assert!(common(&["--lang", "english"]).is_err());
        assert!(common(&["-l", "5"]).is_err());
        assert!(common(&["--lang", "english", "-l", "5"]).is_ok());
    }

    #[test]
    fn a_profile_stands_alone() {
        assert!(common(&["--profile", "wordle"]).is_ok());
    }

    #[test]
    fn a_profile_rejects_language_and_length_even_when_they_agree() {
        assert!(common(&["--profile", "wordle", "-l", "5"]).is_err());
        assert!(common(&["--profile", "wordle", "-l", "6"]).is_err());
        assert!(common(&["--profile", "wordle", "--lang", "english"]).is_err());
    }

    #[test]
    fn a_profile_rejects_dictionary_length() {
        assert!(common(&["--profile", "wordle", "--dictionary-length", "10"]).is_err());
    }

    #[test]
    fn a_profile_plays_every_pool() {
        for (flag, pool) in [
            ("all", GuessPool::All),
            ("hard", GuessPool::Hard),
            ("ultrahard", GuessPool::Ultrahard),
        ] {
            let common = common(&["--profile", "wordle", "--pool", flag]).unwrap();
            assert_eq!(common.strategy().pool, pool);
        }
    }

    #[test]
    fn the_pool_defaults_to_all_and_the_old_flags_are_gone() {
        assert_eq!(
            common(&["--profile", "wordle"]).unwrap().strategy().pool,
            GuessPool::All
        );
        for old in ["--hard", "--ultrahard", "--hard-mode"] {
            assert!(common(&["--profile", "wordle", old]).is_err(), "{old}");
        }
        assert!(common(&["--profile", "wordle", "--pool", "easy"]).is_err());
    }
}

mod track_tests {
    use super::*;
    use crate::config::Targets;

    fn simulate(args: &[&str]) -> Result<SimulateArgs, clap::Error> {
        let Command::Simulate(a) =
            Cli::try_parse_from(["lex", "simulate"].into_iter().chain(args.iter().copied()))?
                .command
        else {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::InvalidSubcommand,
                "`simulate` parsed to another command",
            ));
        };
        Ok(a)
    }

    #[test]
    fn a_track_stands_alone_and_plays_its_config() {
        let args = simulate(&["--track", "wordle-hard"]).unwrap();
        assert_eq!(args.config(), Track::find("wordle-hard").unwrap().config());
        assert_eq!(args.dispatch_length(), 5);
        assert_eq!(
            simulate(&["--track", "russian-8"])
                .unwrap()
                .dispatch_length(),
            8
        );
    }

    #[test]
    fn the_prior_shape_defaults_to_power_and_rejects_bad_values() {
        let base = ["--lang", "english", "-l", "5"];
        let shape = |extra: &[&str]| {
            let args: Vec<&str> = base.iter().chain(extra).copied().collect();
            simulate(&args).map(|a| a.common.strategy().prior_shape)
        };
        assert_eq!(shape(&[]).unwrap(), PriorShape::Power);
        assert_eq!(
            shape(&["--prior-shape", "sigmoid:0.25"]).unwrap(),
            PriorShape::Sigmoid { cutoff: 0.25 }
        );
        assert_eq!(
            shape(&["--prior-shape", "floor:50"]).unwrap(),
            PriorShape::Floor { percentile: 50.0 }
        );
        for bad in ["sigmoid:0", "sigmoid:2", "floor:100", "floor:-5", "bogus"] {
            assert!(shape(&["--prior-shape", bad]).is_err(), "{bad}");
        }
        assert!(simulate(&["--track", "english-5", "--prior-shape", "power"]).is_err());
    }

    #[test]
    fn a_track_conflicts_with_game_pool_and_target_flags() {
        for extra in [
            &["--lang", "english"][..],
            &["-l", "5"],
            &["--profile", "wordle"],
            &["--pool", "hard"],
            &["--temperature", "2"],
            &["-n", "10"],
            &["--seed", "1"],
            &["--targets", "t.txt"],
        ] {
            let args: Vec<&str> = ["--track", "english-5"]
                .into_iter()
                .chain(extra.iter().copied())
                .collect();
            assert!(simulate(&args).is_err(), "{extra:?}");
        }
        assert!(simulate(&["--track", "english-5", "-o", "-"]).is_ok());
        assert!(simulate(&["--track", "english-5ultrahard"]).is_err());
    }

    #[test]
    fn flags_become_a_run_config() {
        let sampled =
            simulate(&["-l", "5", "--lang", "english", "-n", "7", "--seed", "3"]).unwrap();
        assert_eq!(sampled.config().targets, Targets::Sample { n: 7, seed: 3 });
        let full = simulate(&["-l", "5", "--lang", "english", "--pool", "hard"]).unwrap();
        assert_eq!(full.config().targets, Targets::Full);
        assert_eq!(full.config().strategy.pool, GuessPool::Hard);
        let file = simulate(&["-l", "5", "--lang", "english", "--targets", "t.txt"]).unwrap();
        assert_eq!(file.config().targets, Targets::File("t.txt".into()));
        assert!(simulate(&["-l", "5", "--lang", "english", "--targets", "t", "-n", "1"]).is_err());
        assert!(simulate(&["-l", "5", "--lang", "english", "--seed", "1"]).is_err());
    }
}

mod book_tests {
    use super::*;

    fn build(args: &[&str]) -> Result<BookBuildArgs, clap::Error> {
        let Command::Book(BookArgs {
            command: BookCommand::Build(a),
        }) = Cli::try_parse_from(
            ["lex", "book", "build"]
                .into_iter()
                .chain(args.iter().copied()),
        )?
        .command
        else {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::InvalidSubcommand,
                "`book build` parsed to another command",
            ));
        };
        Ok(*a)
    }

    #[test]
    fn build_takes_simulates_game_and_strategy_flags() {
        assert!(build(&[]).is_err());
        let args = build(&["-l", "5", "--lang", "english", "--pool", "hard", "--force"]).unwrap();
        assert!(args.force);
        assert_eq!(args.config().strategy.pool, GuessPool::Hard);
        assert_eq!(args.config().targets, Targets::Full);
        assert_eq!(
            build(&["--profile", "wordle"]).unwrap().dispatch_length(),
            5
        );
    }

    #[test]
    fn build_takes_a_track_alone() {
        let args = build(&["--track", "english-5"]).unwrap();
        assert_eq!(args.config(), Track::find("english-5").unwrap().config());
        assert!(build(&["--track", "english-5", "--pool", "hard"]).is_err());
        assert!(build(&["--track", "english-5", "--force"]).is_ok());
    }

    #[test]
    fn simulate_and_assist_can_opt_out_of_the_book() {
        for command in ["simulate", "assist"] {
            let parse = |extra: &[&str]| {
                Cli::try_parse_from(
                    ["lex", command, "-l", "5", "--lang", "english"]
                        .into_iter()
                        .chain(extra.iter().copied()),
                )
            };
            assert!(parse(&["--no-book"]).is_ok(), "{command}");
        }
    }
}

mod threads_tests {
    use super::*;

    const ASSIST: [&str; 6] = ["assist", "--lang", "english", "-l", "5", "--no-progress"];

    fn threads(args: &[&str]) -> Result<NonZeroUsize, clap::Error> {
        Cli::try_parse_from(std::iter::once("lex").chain(args.iter().copied())).map(|c| c.threads)
    }

    #[test]
    fn threads_accepts_one_and_more() {
        assert_eq!(
            threads(&["--threads", "1", "book", "clear", "--all"])
                .unwrap()
                .get(),
            1
        );
        assert_eq!(
            threads(&["--threads", "7", "book", "clear", "--all"])
                .unwrap()
                .get(),
            7
        );
    }

    #[test]
    fn threads_rejects_zero_and_non_numbers() {
        assert!(threads(&["--threads", "0", "book", "clear", "--all"]).is_err());
        assert!(threads(&["--threads", "-1", "book", "clear", "--all"]).is_err());
        assert!(threads(&["--threads", "many", "book", "clear", "--all"]).is_err());
    }

    #[test]
    fn threads_defaults_to_the_available_parallelism() {
        let default = threads(&ASSIST).unwrap();
        assert!(default.get() >= 1);
        assert_eq!(default, default_threads());
    }

    #[test]
    fn threads_is_global_before_and_after_the_subcommand() {
        let after: Vec<&str> = ASSIST.into_iter().chain(["--threads", "2"]).collect();
        assert_eq!(threads(&after).unwrap().get(), 2);
        let before: Vec<&str> = ["--threads", "3"].into_iter().chain(ASSIST).collect();
        assert_eq!(threads(&before).unwrap().get(), 3);
        assert_eq!(
            threads(&["book", "clear", "--all", "--threads", "4"])
                .unwrap()
                .get(),
            4
        );
    }

    #[test]
    fn the_pool_has_the_requested_width() {
        let pool = build_pool(NonZeroUsize::new(3).unwrap()).unwrap();
        assert_eq!(pool.install(rayon::current_num_threads), 3);
    }

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::parse_args(std::iter::once("lex").chain(args.iter().copied()))
    }

    fn request(args: &[&str]) -> Option<usize> {
        parse(args)
            .unwrap()
            .download_request()
            .map(NonZeroUsize::get)
    }

    #[test]
    fn download_threads_rejects_zero_and_non_numbers() {
        for bad in ["0", "-1", "many"] {
            assert!(parse(&["--download-threads", bad, "book", "clear", "--all"]).is_err());
        }
        assert!(parse(&["--download-threads", "1", "book", "clear", "--all"]).is_ok());
    }

    #[test]
    fn download_threads_wins_over_threads() {
        let args = [
            "--threads",
            "2",
            "--download-threads",
            "9",
            "book",
            "clear",
            "--all",
        ];
        assert_eq!(request(&args), Some(9));
    }

    #[test]
    fn explicit_threads_is_the_download_request() {
        assert_eq!(
            request(&["--threads", "1", "book", "clear", "--all"]),
            Some(1)
        );
        assert_eq!(
            request(&["book", "clear", "--all", "--threads", "6"]),
            Some(6)
        );
    }

    #[test]
    fn neither_option_leaves_the_default() {
        assert_eq!(request(&["book", "clear", "--all"]), None);
        assert!(!parse(&["book", "clear", "--all"]).unwrap().threads_explicit);
    }

    #[test]
    fn download_threads_is_global_before_and_after_the_subcommand() {
        assert_eq!(
            request(&["book", "clear", "--all", "--download-threads", "3"]),
            Some(3)
        );
        assert_eq!(
            request(&["--download-threads", "5", "book", "clear", "--all"]),
            Some(5)
        );
    }
}

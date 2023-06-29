use super::Object;
use anyhow::Result;
use clap::{crate_version, ArgAction, Parser};
use serde::{Deserialize, Serialize};
use std::{env, ffi::OsStr};

#[derive(Debug, Parser)]
#[command(bin_name = "cargo")]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Debug, Parser)]
enum SubCommand {
    TestFuzz(TestFuzzWithDeprecations),
}

// smoelius: Wherever possible, try to reuse cargo test and libtest option names.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Deserialize, Parser, Serialize)]
#[command(version = crate_version!(), after_help = "To fuzz at most <SECONDS> of time, use:

    cargo test-fuzz ... -- -V <SECONDS>

Try `cargo afl fuzz --help` to see additional fuzzer options.
")]
#[remain::sorted]
struct TestFuzzWithDeprecations {
    #[arg(long, help = "Display backtraces")]
    backtrace: bool,
    #[arg(
        long,
        help = "Move one target's crashes, hangs, and work queue to its corpus; to consolidate \
        all targets, use --consolidate-all"
    )]
    consolidate: bool,
    #[arg(long, hide = true)]
    consolidate_all: bool,
    #[arg(
        long,
        value_name = "OBJECT",
        help = "Display concretizations, corpus, crashes, `impl` concretizations, hangs, or work \
        queue. By default, corpus uses an uninstrumented fuzz target; the others use an \
        instrumented fuzz target. To display the corpus with instrumentation, use --display \
        corpus-instrumented."
    )]
    display: Option<Object>,
    #[arg(long, help = "Target name is an exact name rather than a substring")]
    exact: bool,
    #[arg(
        long,
        help = "Exit with 0 if the time limit was reached, 1 for other programmatic aborts, and 2 \
        if an error occurred; implies --no-ui, does not imply --run-until-crash or -- -V <SECONDS>"
    )]
    exit_code: bool,
    #[arg(
        long,
        action = ArgAction::Append,
        help = "Space or comma separated list of features to activate"
    )]
    features: Vec<String>,
    #[arg(long, help = "List fuzz targets")]
    list: bool,
    #[arg(long, value_name = "PATH", help = "Path to Cargo.toml")]
    manifest_path: Option<String>,
    #[arg(long, help = "Do not activate the `default` feature")]
    no_default_features: bool,
    #[arg(
        long,
        help = "Compile without instrumentation (for testing build process)"
    )]
    no_instrumentation: bool,
    #[arg(long, help = "Compile, but don't fuzz")]
    no_run: bool,
    #[arg(long, help = "Disable user interface")]
    no_ui: bool,
    #[arg(short, long, help = "Package containing fuzz target")]
    package: Option<String>,
    #[arg(long, help = "Enable persistent mode fuzzing")]
    persistent: bool,
    #[arg(long, help = "Pretty-print debug output when displaying/replaying")]
    pretty_print: bool,
    #[arg(
        long,
        value_name = "OBJECT",
        help = "Replay corpus, crashes, hangs, or work queue. By default, corpus uses an \
        uninstrumented fuzz target; the others use an instrumented fuzz target. To replay the \
        corpus with instrumentation, use --replay corpus-instrumented."
    )]
    replay: Option<Object>,
    #[arg(
        long,
        help = "Clear fuzzing data for one target, but leave corpus intact; to reset all \
        targets, use --reset-all"
    )]
    reset: bool,
    #[arg(long, hide = true)]
    reset_all: bool,
    #[arg(long, help = "Resume target's last fuzzing session")]
    resume: bool,
    #[arg(long, help = "Stop fuzzing once a crash is found")]
    run_until_crash: bool,
    #[arg(
        long,
        value_name = "NAME",
        help = "Integration test containing fuzz target"
    )]
    test: Option<String>,
    #[arg(
        long,
        help = "Number of seconds to consider a hang when fuzzing or replaying (equivalent \
        to -- -t <TIMEOUT * 1000> when fuzzing)"
    )]
    timeout: Option<u64>,
    #[arg(long, help = "Show build output when displaying/replaying")]
    verbose: bool,
    #[arg(
        value_name = "TARGETNAME",
        help = "String that fuzz target's name must contain"
    )]
    ztarget: Option<String>,
    #[arg(last = true, name = "ARGS", help = "Arguments for the fuzzer")]
    zzargs: Vec<String>,
}

impl From<TestFuzzWithDeprecations> for super::TestFuzz {
    fn from(opts: TestFuzzWithDeprecations) -> Self {
        let TestFuzzWithDeprecations {
            backtrace,
            consolidate,
            consolidate_all,
            display,
            exact,
            exit_code,
            features,
            list,
            manifest_path,
            no_default_features,
            no_instrumentation,
            no_run,
            no_ui,
            package,
            persistent,
            pretty_print,
            replay,
            reset,
            reset_all,
            resume,
            run_until_crash,
            test,
            timeout,
            verbose,
            ztarget,
            zzargs,
        } = opts;
        Self {
            backtrace,
            consolidate,
            consolidate_all,
            display,
            exact,
            exit_code,
            features,
            list,
            manifest_path,
            no_default_features,
            no_instrumentation,
            no_run,
            no_ui,
            package,
            persistent,
            pretty_print,
            replay,
            reset,
            reset_all,
            resume,
            run_until_crash,
            test,
            timeout,
            verbose,
            ztarget,
            zzargs,
        }
    }
}

#[allow(unused_macros)]
macro_rules! process_deprecated_action_object {
    ($opts:ident, $action:ident, $object:ident) => {
        paste::paste! {
            if $opts.[< $action _ $object >] {
                use heck::ToKebabCase;
                eprintln!(
                    "`--{}-{}` is deprecated. Use `--{} {}` (no hyphen).",
                    stringify!($action),
                    stringify!($object).to_kebab_case(),
                    stringify!($action),
                    stringify!($object).to_kebab_case(),
                );
                if $opts.$action.is_none() {
                    $opts.$action = Some(Object::[< $object:camel >]);
                }
            }
        }
    };
}

pub(crate) fn cargo_test_fuzz<T: AsRef<OsStr>>(args: &[T]) -> Result<()> {
    #[allow(unused_mut)]
    let SubCommand::TestFuzz(mut opts) = Opts::parse_from(args).subcmd;

    super::run(super::TestFuzz::from(opts))
}

#[test]
fn verify_cli() {
    use clap::CommandFactory;
    Opts::command().debug_assert();
}
#![deny(clippy::expect_used)]
#![deny(clippy::unwrap_used)]
#![warn(clippy::panic)]

use anyhow::{ensure, Result};
use cargo_metadata::{Artifact, ArtifactProfile, Message};
use clap::Clap;
use dirs::{
    corpus_directory_from_target, crashes_directory_from_target, output_directory_from_target,
    queue_directory_from_target,
};
use log::debug;
use std::{
    ffi::OsStr,
    fmt::Debug,
    fs::{create_dir_all, read_dir, File},
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    process::Command,
};
use subprocess::{Exec, NullFile, Redirection};

const ENTRY_SUFFIX: &str = "_fuzz::entry";

#[derive(Clap, Debug)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Clap, Debug)]
enum SubCommand {
    TestFuzz(TestFuzz),
}

// smoelius: Wherever possible, try to reuse cargo test and libtest option names.
#[derive(Clap, Debug)]
struct TestFuzz {
    #[clap(long, about = "Display backtraces")]
    backtrace: bool,
    #[clap(
        long,
        about = "Display corpus using uninstrumented fuzz target; to display with instrumentation, \
use --display-corpus-instrumented"
    )]
    display_corpus: bool,
    #[clap(long, hidden = true)]
    display_corpus_instrumented: bool,
    #[clap(long, about = "Display crashes")]
    display_crashes: bool,
    #[clap(long, about = "Display work queue")]
    display_queue: bool,
    #[clap(long, about = "Target name is an exact name rather than a substring")]
    exact: bool,
    #[clap(long, about = "List fuzz targets")]
    list: bool,
    #[clap(long, about = "Resume target's last fuzzing session")]
    resume: bool,
    #[clap(
        long,
        about = "Compile without instrumentation (for testing build process)"
    )]
    no_instrumentation: bool,
    #[clap(long, about = "Compile, but don't fuzz")]
    no_run: bool,
    #[clap(long, about = "Disable user interface")]
    no_ui: bool,
    #[clap(long, about = "Enable persistent mode fuzzing")]
    persistent: bool,
    #[clap(long, about = "Pretty-print debug output")]
    pretty_print: bool,
    #[clap(short, long, about = "Package containing fuzz target")]
    package: Option<String>,
    #[clap(
        long,
        about = "Replay corpus using uninstrumented fuzz target; to replay with instrumentation, \
use --replay-corpus-instrumented"
    )]
    replay_corpus: bool,
    #[clap(long, hidden = true)]
    replay_corpus_instrumented: bool,
    #[clap(long, about = "Replay crashes")]
    replay_crashes: bool,
    #[clap(long, about = "Replay work queue")]
    replay_queue: bool,
    #[clap(long, about = "String that fuzz target's name must contain")]
    target: Option<String>,
    #[clap(last = true, about = "Arguments for the fuzzer")]
    args: Vec<String>,
}

pub fn cargo_test_fuzz<T: AsRef<OsStr>>(args: &[T]) -> Result<()> {
    let opts = {
        let SubCommand::TestFuzz(mut opts) = Opts::parse_from(args).subcmd;
        if opts.display_corpus || opts.replay_corpus {
            opts.no_instrumentation = true;
        }
        opts
    };

    let executables = build(&opts)?;

    let mut executable_targets = executable_targets(&executables)?;

    if let Some(pat) = &opts.target {
        executable_targets = filter_executable_targets(&opts, &pat, &executable_targets);
    }

    if opts.list {
        println!("{:#?}", executable_targets);
        return Ok(());
    }

    if opts.no_run {
        return Ok(());
    }

    let (executable, krate, target) = executable_target(&opts, &executable_targets)?;

    let display = opts.display_corpus
        || opts.display_corpus_instrumented
        || opts.display_crashes
        || opts.display_queue;

    let replay = opts.replay_corpus
        || opts.replay_corpus_instrumented
        || opts.replay_crashes
        || opts.replay_queue;

    let dir = if opts.display_corpus || opts.display_corpus_instrumented || opts.replay_corpus {
        corpus_directory_from_target(&krate, &target)
    } else if opts.display_crashes || opts.replay_crashes {
        crashes_directory_from_target(&krate, &target)
    } else if opts.display_queue || opts.replay_queue {
        queue_directory_from_target(&krate, &target)
    } else {
        PathBuf::default()
    };

    if display {
        return for_each_entry(&opts, &executable, &krate, &target, Action::Display, &dir);
    } else if replay {
        return for_each_entry(&opts, &executable, &krate, &target, Action::Replay, &dir);
    }

    if opts.no_instrumentation {
        println!("Stopping before fuzzing since --no-instrumentation was specified.");
        return Ok(());
    }

    fuzz(&opts, &executable, &krate, &target)
}

fn build(opts: &TestFuzz) -> Result<Vec<(PathBuf, String)>> {
    // smoelius: Put --message-format=json last so that it is easy to copy-and-paste the command
    // without it.
    let mut args = vec![];
    if !opts.no_instrumentation {
        args.extend_from_slice(&["afl"]);
    }
    args.extend_from_slice(&["test", "--no-run"]);
    if let Some(package) = &opts.package {
        args.extend_from_slice(&["--package", &package])
    }
    if opts.persistent {
        args.extend_from_slice(&["--features", "test-fuzz/persistent"]);
    }
    args.extend_from_slice(&["--message-format=json"]);

    let exec = Exec::cmd("cargo").args(&args);
    debug!("{:?}", exec);
    let stream = exec.stream_stdout()?;
    let reader = BufReader::new(stream);

    let messages: Vec<Message> =
        Message::parse_stream(reader).collect::<std::result::Result<_, std::io::Error>>()?;

    Ok(messages
        .into_iter()
        .filter_map(|message| {
            if let Message::CompilerArtifact(Artifact {
                target: build_target,
                profile: ArtifactProfile { test: true, .. },
                executable: Some(executable),
                ..
            }) = message
            {
                Some((executable, build_target.name))
            } else {
                None
            }
        })
        .collect())
}

fn executable_targets(
    executables: &[(PathBuf, String)],
) -> Result<Vec<(PathBuf, String, Vec<String>)>> {
    let executable_targets: Vec<(PathBuf, String, Vec<String>)> = executables
        .iter()
        .map(|(executable, krate)| {
            let targets = targets(executable)?;
            Ok((executable.clone(), krate.clone(), targets))
        })
        .collect::<Result<_>>()?;

    Ok(executable_targets
        .into_iter()
        .filter(|executable_targets| !executable_targets.2.is_empty())
        .collect())
}

fn targets(executable: &PathBuf) -> Result<Vec<String>> {
    let exec = Exec::cmd(executable).args(&["--list"]);
    debug!("{:?}", exec);
    let stream = exec.stream_stdout()?;

    // smoelius: A test executable's --list output ends with an empty line followed by
    // "M tests, N benchmarks." Stop at the empty line.
    let mut targets = Vec::<String>::default();
    for line in BufReader::new(stream).lines() {
        let line = line?;
        if line.is_empty() {
            break;
        }
        let line = if let Some(line) = line.strip_suffix(": test") {
            line
        } else {
            continue;
        };
        let line = if let Some(line) = line.strip_suffix(ENTRY_SUFFIX) {
            line
        } else {
            continue;
        };
        targets.push(line.to_owned());
    }
    Ok(targets)
}

fn filter_executable_targets(
    opts: &TestFuzz,
    pat: &str,
    executable_targets: &[(PathBuf, String, Vec<String>)],
) -> Vec<(PathBuf, String, Vec<String>)> {
    executable_targets
        .iter()
        .filter_map(|(executable, krate, targets)| {
            let targets = filter_targets(opts, pat, targets);
            if !targets.is_empty() {
                Some((executable.clone(), krate.clone(), targets))
            } else {
                None
            }
        })
        .collect()
}

fn filter_targets(opts: &TestFuzz, pat: &str, targets: &[String]) -> Vec<String> {
    targets
        .iter()
        .filter(|target| (!opts.exact && target.contains(pat)) || target.as_str() == pat)
        .cloned()
        .collect()
}

fn executable_target(
    opts: &TestFuzz,
    executable_targets: &[(PathBuf, String, Vec<String>)],
) -> Result<(PathBuf, String, String)> {
    let mut executable_targets = executable_targets.to_vec();

    ensure!(
        !executable_targets.is_empty(),
        "found no fuzz targets{}",
        match_message(opts)
    );

    ensure!(
        executable_targets.len() <= 1,
        "found multiple executables with fuzz targets{}: {:#?}",
        match_message(opts),
        executable_targets
    );

    let mut executable_targets = executable_targets.remove(0);

    assert!(!executable_targets.2.is_empty());

    ensure!(
        executable_targets.2.len() <= 2,
        "found multiple fuzz targets{} in {:?}: {:#?}",
        match_message(opts),
        (executable_targets.0, executable_targets.1),
        executable_targets.2
    );

    Ok((
        executable_targets.0,
        executable_targets.1,
        executable_targets.2.remove(0),
    ))
}

fn match_message(opts: &TestFuzz) -> String {
    opts.target.as_ref().map_or("".to_owned(), |pat| {
        format!(
            " {} `{}`",
            if opts.exact { "equal to" } else { "containing" },
            pat
        )
    })
}

#[derive(Eq, PartialEq)]
enum Action {
    Display,
    Replay,
}

fn for_each_entry(
    opts: &TestFuzz,
    executable: &PathBuf,
    _krate: &str,
    target: &str,
    action: Action,
    dir: &PathBuf,
) -> Result<()> {
    let mut env = vec![("TEST_FUZZ", "1")];
    if opts.backtrace {
        env.extend(&[("RUST_BACKTRACE", "1")]);
    }
    env.extend(&[(
        match action {
            Action::Display => "TEST_FUZZ_DISPLAY",
            Action::Replay => "TEST_FUZZ_REPLAY",
        },
        "1",
    )]);
    if opts.pretty_print {
        env.extend(&[("TEST_FUZZ_PRETTY_PRINT", "1")]);
    }

    let args: Vec<String> = vec![
        "--exact",
        &(target.to_owned() + ENTRY_SUFFIX),
        "--nocapture",
    ]
    .into_iter()
    .map(String::from)
    .collect();

    let mut nonempty = false;
    let mut failure = false;
    let mut output = false;

    for entry in read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file = File::open(&path)?;
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();

        if file_name == "README.txt" || file_name == ".state" {
            continue;
        }

        let exec = Exec::cmd(executable)
            .env_extend(&env)
            .args(&args)
            .stdin(file)
            .stdout(NullFile)
            .stderr(Redirection::Pipe);
        debug!("{:?}", exec);
        let mut popen = exec.popen()?;
        let buffer = popen
            .stderr
            .as_mut()
            .map_or(Ok(vec![]), |stderr| -> Result<_> {
                let mut buffer = Vec::new();
                stderr.read_to_end(&mut buffer)?;
                Ok(buffer)
            })?;
        let status = popen.wait()?;

        print!("{}: ", file_name);
        if buffer.is_empty() {
            println!("{:?}", status);
        } else {
            print!("{}", String::from_utf8_lossy(&buffer));
            output = true;
        }

        failure |= !status.success();

        nonempty = true;
    }

    assert!(!(!nonempty && (failure || output)));

    if !nonempty {
        println!(
            "Nothing to {}.",
            match action {
                Action::Display => "display",
                Action::Replay => "replay",
            }
        );
        return Ok(());
    }

    if !failure && !output {
        println!("No output on stderr detected.");
        return Ok(());
    }

    if failure && action == Action::Display {
        println!(
            "Encountered a failure while not replaying. A buggy Debug implementation perhaps?"
        );
        return Ok(());
    }

    Ok(())
}

fn fuzz(opts: &TestFuzz, executable: &PathBuf, krate: &str, target: &str) -> Result<()> {
    let corpus = corpus_directory_from_target(krate, target)
        .to_string_lossy()
        .into_owned();

    let output = output_directory_from_target(krate, target);
    create_dir_all(&output).unwrap_or_default();

    let mut command = Command::new("cargo");

    let mut env = vec![("TEST_FUZZ", "1")];
    if opts.no_ui {
        env.extend(&[("AFL_NO_UI", "1")]);
    }

    let mut args = vec![];
    args.extend(
        vec![
            "afl",
            "fuzz",
            "-i",
            if opts.resume { "-" } else { &corpus },
            "-o",
            &output.to_string_lossy(),
        ]
        .into_iter()
        .map(String::from),
    );
    args.extend(opts.args.clone());
    args.extend(
        vec![
            "--",
            &executable.to_string_lossy(),
            "--exact",
            &(target.to_owned() + ENTRY_SUFFIX),
        ]
        .into_iter()
        .map(String::from),
    );

    command.envs(env).args(args);

    let status = command.status()?;

    ensure!(status.success(), "command failed: {:?}", command);

    Ok(())
}
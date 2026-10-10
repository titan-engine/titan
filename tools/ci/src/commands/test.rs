use crate::{args::Args, Prepare, PreparedCommand};
use argh::FromArgs;
use xshell::cmd;

/// Runs all tests (except for doc tests).
#[derive(FromArgs, Default)]
#[argh(subcommand, name = "test")]
pub struct TestCommand {
    /// compile tests and benchmarks without running them
    #[argh(switch)]
    pub no_run: bool,

    /// skip compiling and running benchmarks
    #[argh(switch)]
    pub skip_benches: bool,
}

impl Prepare for TestCommand {
    fn prepare<'a>(&self, sh: &'a xshell::Shell, args: Args) -> Vec<PreparedCommand<'a>> {
        let no_fail_fast = args.keep_going();
        let no_run = self.no_run.then_some("--no-run");
        let jobs = args.build_jobs();
        let test_threads = args.test_threads();

        let jobs_ref = &jobs;
        let test_threads_ref = &test_threads;

        // The bevy_ecs error tests need this set to test backtraces
        sh.set_var("RUST_BACKTRACE", "1");

        let mut commands = vec![PreparedCommand::new::<Self>(
            cmd!(
                sh,
                "cargo test --workspace --lib --bins --tests --features bevy_ecs/track_location {no_run...} {no_fail_fast...} {jobs_ref...} -- {test_threads_ref...}"
            ),
            "Please fix failing tests in output above.",
        )];

        if !self.skip_benches {
            commands.push(PreparedCommand::new::<Self>(
                cmd!(
                    sh,
                    // `--benches` runs each benchmark once in order to verify that they behave
                    // correctly and do not panic.
                    "cargo test --workspace --benches {no_run...} {no_fail_fast...} {jobs...}"
                ),
                "Please fix failing tests in output above.",
            ));
        }

        commands
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CI;

    #[test]
    fn benches_run_by_default() {
        let ci = CI::from_args(&["ci"], &[]).unwrap();
        let sh = xshell::Shell::new().unwrap();
        let commands = TestCommand::default().prepare(&sh, (&ci).into());

        assert_eq!(commands.len(), 2);
        assert_eq!(
            commands[0].command.to_string(),
            "cargo test --workspace --lib --bins --tests --features bevy_ecs/track_location --"
        );
        assert_eq!(
            commands[1].command.to_string(),
            "cargo test --workspace --benches"
        );
        assert_eq!(sh.var("RUST_BACKTRACE").unwrap(), "1");
    }

    #[test]
    fn skip_benches_preserves_test_command() {
        let ci = CI::from_args(&["ci"], &[]).unwrap();
        let sh = xshell::Shell::new().unwrap();
        let default_commands = TestCommand::default().prepare(&sh, (&ci).into());
        let command = TestCommand::from_args(&["ci", "test"], &["--skip-benches"]).unwrap();
        let commands = command.prepare(&sh, (&ci).into());

        assert_eq!(commands.len(), 1);
        assert_eq!(
            commands[0].command.to_string(),
            default_commands[0].command.to_string()
        );
        assert_eq!(sh.var("RUST_BACKTRACE").unwrap(), "1");
    }

    #[test]
    fn no_run_warms_the_same_test_and_bench_targets() {
        let ci = CI::from_args(&["ci"], &[]).unwrap();
        let sh = xshell::Shell::new().unwrap();
        let command = TestCommand::from_args(&["ci", "test"], &["--no-run"]).unwrap();
        let commands = command.prepare(&sh, (&ci).into());

        assert_eq!(commands.len(), 2);
        assert_eq!(
            commands[0].command.to_string(),
            "cargo test --workspace --lib --bins --tests --features bevy_ecs/track_location --no-run --"
        );
        assert_eq!(
            commands[1].command.to_string(),
            "cargo test --workspace --benches --no-run"
        );
        let command =
            TestCommand::from_args(&["ci", "test"], &["--no-run", "--skip-benches"]).unwrap();
        assert_eq!(command.prepare(&sh, (&ci).into()).len(), 1);
    }

    #[test]
    fn global_flags_are_preserved() {
        let ci = CI::from_args(
            &["ci"],
            &[
                "--keep-going",
                "--build-jobs",
                "2",
                "--test-threads",
                "3",
                "test",
                "--skip-benches",
            ],
        )
        .unwrap();
        let sh = xshell::Shell::new().unwrap();
        let commands = TestCommand::default().prepare(&sh, (&ci).into());

        assert_eq!(
            commands[0].command.to_string(),
            "cargo test --workspace --lib --bins --tests --features bevy_ecs/track_location --no-fail-fast --jobs=2 -- --test-threads=3"
        );
        assert_eq!(
            commands[1].command.to_string(),
            "cargo test --workspace --benches --no-fail-fast --jobs=2"
        );

        let command = TestCommand::from_args(&["ci", "test"], &["--skip-benches"]).unwrap();
        let skipped_commands = command.prepare(&sh, (&ci).into());
        assert_eq!(skipped_commands.len(), 1);
        assert_eq!(
            skipped_commands[0].command.to_string(),
            commands[0].command.to_string()
        );
    }
}

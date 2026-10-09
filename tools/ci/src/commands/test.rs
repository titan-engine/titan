use crate::{args::Args, Prepare, PreparedCommand};
use argh::FromArgs;
use xshell::cmd;

/// Runs all tests (except for doc tests).
#[derive(FromArgs, Default)]
#[argh(subcommand, name = "test")]
pub struct TestCommand {
    /// skip compiling and running benchmarks
    #[argh(switch)]
    pub skip_benches: bool,
}

impl Prepare for TestCommand {
    fn prepare<'a>(&self, sh: &'a xshell::Shell, args: Args) -> Vec<PreparedCommand<'a>> {
        let no_fail_fast = args.keep_going();
        let jobs = args.build_jobs();
        let test_threads = args.test_threads();

        let jobs_ref = &jobs;
        let test_threads_ref = &test_threads;

        // The bevy_ecs error tests need this set to test backtraces
        sh.set_var("RUST_BACKTRACE", "1");

        let mut commands = vec![PreparedCommand::new::<Self>(
            cmd!(
                sh,
                "cargo test --workspace --lib --bins --tests --features bevy_ecs/track_location {no_fail_fast...} {jobs_ref...} -- {test_threads_ref...}"
            ),
            "Please fix failing tests in output above.",
        )];

        if !self.skip_benches {
            commands.push(PreparedCommand::new::<Self>(
                cmd!(
                    sh,
                    // `--benches` runs each benchmark once in order to verify that they behave
                    // correctly and do not panic.
                    "cargo test --workspace --benches {no_fail_fast...} {jobs...}"
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

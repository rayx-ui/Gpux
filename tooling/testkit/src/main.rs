//! `gpux-testkit`: runs gpux's test lanes. See [`gpux_testkit`] for the commands.

fn main() {
    let code = gpux_testkit::execute(
        std::env::args().skip(1),
        &gpux_testkit::SystemEnvironment,
        &mut gpux_testkit::run::ProcessRunner::default(),
        &mut std::io::stdout(),
    );
    std::process::exit(code);
}

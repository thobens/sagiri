//! Filter mode: all of stdin in, obfuscated text out.

use std::io::{self, Read, Write};

use anyhow::{Context as _, anyhow};
use sagiri_core::engine::Engine;

/// Reads all of `input`, writes the obfuscated text to `output`.
///
/// The output is exactly the transformed text. A broken pipe on `output` is not an error.
pub fn run(engine: &Engine, mut input: impl Read, mut output: impl Write) -> anyhow::Result<()> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).context("cannot read input")?;
    let text = String::from_utf8(bytes).map_err(|_| anyhow!("input is not valid UTF-8"))?;
    let result = engine.obfuscate(&text);
    match output
        .write_all(result.as_bytes())
        .and_then(|()| output.flush())
    {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        r => r.context("cannot write output"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sagiri_core::key::Key;
    use sagiri_core::pseudo::Pseudonymizer;
    use sagiri_core::rules::default_rules;

    fn engine() -> Engine {
        Engine::new(
            Pseudonymizer::new(&Key::from_bytes([7; 32])),
            default_rules(),
        )
    }

    struct Failing(io::ErrorKind);
    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(self.0.into())
        }
    }

    #[test]
    fn broken_pipe_is_ok() {
        let out = Failing(io::ErrorKind::BrokenPipe);
        assert!(run(&engine(), &b"a 172.55.0.1 b\n"[..], out).is_ok());
    }

    #[test]
    fn other_write_errors_are_reported() {
        let out = Failing(io::ErrorKind::PermissionDenied);
        assert!(run(&engine(), &b"x"[..], out).is_err());
    }
}

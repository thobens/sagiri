//! The clipboard holder: a second `sagiri-cli` process that owns a selection on Linux.
//!
//! On Linux a selection is served by the process that set it and is gone when that process
//! exits. `sagiri-cli --out clipboard` therefore starts itself again with `--hold
//! <selection>`, hands the text over a pipe and returns once the holder confirms with `ok`.
//! The holder serves the selection until another program takes it over.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Context as _;

use crate::clipboard::{Selection, SystemClipboard};
use crate::filter;

/// The parent's error whenever the holder does not confirm.
const NOT_SET: &str = "the clipboard could not be set";

/// The line the holder writes once it owns the selection.
const ACK: &str = "ok";

/// Owning a selection, in the two steps the holder needs.
pub trait Hold {
    /// Takes the selection with `text` and returns once it is owned.
    fn take(&mut self, selection: Selection, text: &str) -> anyhow::Result<()>;
    /// Serves `text` on the selection until another owner replaces it.
    fn serve_until_replaced(&mut self, selection: Selection, text: &str) -> anyhow::Result<()>;
}

/// The real selection, through arboard.
///
/// `take` is a normal write: arboard's own thread owns and serves the selection from then
/// on. `serve_until_replaced` writes the same text again and blocks until the selection is
/// taken over; re-setting a selection the process already owns does not end that wait.
#[derive(Default)]
pub struct SystemHold {
    clipboard: SystemClipboard,
}

impl Hold for SystemHold {
    fn take(&mut self, selection: Selection, text: &str) -> anyhow::Result<()> {
        crate::clipboard::SelectionClipboard::write_text(&mut self.clipboard, selection, text)
    }

    fn serve_until_replaced(&mut self, selection: Selection, text: &str) -> anyhow::Result<()> {
        self.clipboard.write_text_and_wait(selection, text)
    }
}

/// The value of `--hold` for `selection`.
fn selection_name(selection: Selection) -> &'static str {
    match selection {
        Selection::Clipboard => "clipboard",
        Selection::Primary => "primary",
    }
}

/// The holder command: `exe --hold <selection>`, stdin and stdout piped, stderr discarded,
/// in its own process group. The text is never part of the command.
pub fn command(exe: &Path, selection: Selection) -> Command {
    let mut command = Command::new(exe);
    command
        .arg("--hold")
        .arg(selection_name(selection))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    command
}

/// Starts `command`, writes `text` to its stdin, closes it and reads one line from its
/// stdout. `ok` is success; anything else, or end of input, is an error. Returns without
/// waiting for the child to exit.
pub fn spawn_with(mut command: Command, text: &str) -> anyhow::Result<()> {
    command.stdin(Stdio::piped()).stdout(Stdio::piped());
    let mut child = command.spawn().context(NOT_SET)?;
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        anyhow::bail!(NOT_SET);
    };
    // A child that exits early closes the pipe; the missing `ok` below reports that.
    let written = stdin
        .write_all(text.as_bytes())
        .and_then(|()| stdin.flush());
    drop(stdin);
    let mut line = String::new();
    let read = BufReader::new(stdout).read_line(&mut line);
    // The holder keeps running until the selection is replaced; it is not waited for.
    drop(child);
    match (written, read) {
        (Ok(()), Ok(_)) if line.trim_end_matches('\n') == ACK => Ok(()),
        _ => anyhow::bail!(NOT_SET),
    }
}

/// Hands `text` to a new holder process for `selection`.
pub fn spawn(selection: Selection, text: &str) -> anyhow::Result<()> {
    let exe = std::env::current_exe().context(NOT_SET)?;
    spawn_with(command(&exe, selection), text)
}

/// The holder's work: reads the text from `input` to its end, takes the selection, writes
/// `ok` to `ack` and drops it, then serves the selection until it is replaced. If reading
/// or taking fails, nothing is written to `ack`.
///
/// Dropping `ack` does not close the process's fd 1 when `ack` is stdout: std has no safe
/// way to do that. It does no harm, because the parent reads only the `ok` line and closes
/// its end of the pipe.
pub fn serve(
    input: impl Read,
    mut ack: impl Write,
    hold: &mut dyn Hold,
    selection: Selection,
) -> anyhow::Result<()> {
    let text = filter::read_text(input)?;
    hold.take(selection, &text)?;
    ack.write_all(format!("{ACK}\n").as_bytes())
        .and_then(|()| ack.flush())
        .context("cannot confirm the clipboard")?;
    drop(ack);
    // A known, short race: if another program takes the selection between `take` and this
    // second write, the second write takes it back. arboard 3.7 offers no other way to
    // learn that ownership was taken (`WaitConfig` is None, Forever, Until or Once).
    hold.serve_until_replaced(selection, &text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    type Log = Rc<RefCell<Vec<String>>>;

    /// Records every call, with the selection and the text, in a shared log.
    struct FakeHold {
        log: Log,
        fail_take: bool,
    }

    impl Hold for FakeHold {
        fn take(&mut self, selection: Selection, text: &str) -> anyhow::Result<()> {
            self.log
                .borrow_mut()
                .push(format!("take {selection:?} {text:?}"));
            if self.fail_take {
                anyhow::bail!("fake take failure");
            }
            Ok(())
        }
        fn serve_until_replaced(&mut self, selection: Selection, text: &str) -> anyhow::Result<()> {
            self.log
                .borrow_mut()
                .push(format!("serve {selection:?} {text:?}"));
            Ok(())
        }
    }

    /// An `ack` writer that appends what it receives to the same log.
    struct AckLog(Log);

    impl Write for AckLog {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let text = String::from_utf8_lossy(buf).into_owned();
            self.0.borrow_mut().push(format!("ack {text:?}"));
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn fake(fail_take: bool) -> (FakeHold, Log) {
        let log = Log::default();
        let hold = FakeHold {
            log: log.clone(),
            fail_take,
        };
        (hold, log)
    }

    #[test]
    fn serve_acks_after_taking_the_selection() {
        for selection in [Selection::Clipboard, Selection::Primary] {
            let (mut hold, log) = fake(false);
            serve(
                &b"host 172.55.0.1"[..],
                AckLog(log.clone()),
                &mut hold,
                selection,
            )
            .unwrap();
            let expected = vec![
                format!(r#"take {selection:?} "host 172.55.0.1""#),
                r#"ack "ok\n""#.to_string(),
                format!(r#"serve {selection:?} "host 172.55.0.1""#),
            ];
            assert_eq!(*log.borrow(), expected);
        }
    }

    #[test]
    fn serve_gives_no_ack_when_take_fails() {
        let (mut hold, log) = fake(true);
        let mut ack = Vec::new();
        let r = serve(
            &b"host 172.55.0.1"[..],
            &mut ack,
            &mut hold,
            Selection::Clipboard,
        );
        assert!(r.is_err());
        assert!(ack.is_empty());
        assert_eq!(
            *log.borrow(),
            vec![r#"take Clipboard "host 172.55.0.1""#.to_string()]
        );
    }

    #[test]
    fn serve_rejects_invalid_utf8() {
        let (mut hold, log) = fake(false);
        let mut ack = Vec::new();
        let r = serve(
            &b"a\xff\xfeSECRET"[..],
            &mut ack,
            &mut hold,
            Selection::Clipboard,
        );
        let msg = format!("{:#}", r.unwrap_err());
        assert_eq!(msg, "input is not valid UTF-8");
        assert!(ack.is_empty());
        assert!(log.borrow().is_empty());
    }

    /// `sh -c script` with stderr discarded, as `command(..)` does it: a child that keeps
    /// running would otherwise hold the test runner's output pipe open.
    fn sh(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.arg("-c").arg(script).stderr(Stdio::null());
        command
    }

    #[test]
    fn spawn_succeeds_on_ack() {
        spawn_with(sh("cat >/dev/null; echo ok"), "host 172.55.0.1\n").unwrap();
    }

    #[test]
    fn spawn_fails_when_child_exits_without_ack() {
        let err = spawn_with(sh("cat >/dev/null"), "host 172.55.0.1\n").unwrap_err();
        assert_eq!(format!("{err:#}"), NOT_SET);
        let err = spawn_with(sh("cat >/dev/null; echo nope"), "x").unwrap_err();
        assert_eq!(format!("{err:#}"), NOT_SET);
    }

    #[test]
    fn spawn_does_not_wait_for_the_child() {
        let start = Instant::now();
        spawn_with(sh("cat >/dev/null; echo ok; sleep 5"), "host 172.55.0.1").unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn command_carries_no_text() {
        for (selection, name) in [
            (Selection::Clipboard, "clipboard"),
            (Selection::Primary, "primary"),
        ] {
            let command = command(Path::new("/usr/bin/sagiri-cli"), selection);
            assert_eq!(command.get_program(), "/usr/bin/sagiri-cli");
            let args: Vec<_> = command.get_args().collect();
            assert_eq!(args, ["--hold", name]);
            assert_eq!(command.get_envs().count(), 0);
        }
    }
}

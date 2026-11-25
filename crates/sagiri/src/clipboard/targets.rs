//! Asks the owner of an X11 selection which targets it offers (the ICCCM `TARGETS`
//! request), so that an owner without text can be told apart from one whose text read
//! fails. Some owners, such as `xclip -t image/png`, answer a text request with data of
//! another type; arboard reports that as an error, not as missing content.
//!
//! The query uses its own connection and window, and reads only the list of targets,
//! never the content of the selection.

use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, CURRENT_TIME, NONE};

use super::{Offer, Selection};

/// The targets arboard asks for when it reads text. An owner that lists one of them
/// offers text.
const TEXT_TARGETS: [&[u8]; 6] = [
    b"UTF8_STRING",
    b"text/plain;charset=utf-8",
    b"text/plain;charset=UTF-8",
    b"STRING",
    b"TEXT",
    b"text/plain",
];

/// The property on the query window that receives the list of targets.
const PROPERTY: &[u8] = b"SAGIRI_TARGETS";

/// How long the owner has to answer. An owner that does not answer in time counts as one
/// that does not answer `TARGETS` at all.
const TIMEOUT: Duration = Duration::from_secs(1);

/// What the owner of `selection` offers. `Offer::Unknown` unless arboard is certain to use
/// X11, that is unless `WAYLAND_DISPLAY` is unset (arboard tries Wayland whenever it is
/// set, even when empty), and whenever the query fails, the selection has no owner or the
/// owner does not answer.
pub fn offer(selection: Selection) -> Offer {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return Offer::Unknown;
    }
    query(selection).unwrap_or(Offer::Unknown)
}

/// `Offer::Text` if `listed` contains one of `text`, otherwise `Offer::NoText`.
fn decide(listed: &[Atom], text: &[Atom]) -> Offer {
    if listed
        .iter()
        .any(|atom| *atom != NONE && text.contains(atom))
    {
        Offer::Text
    } else {
        Offer::NoText
    }
}

fn intern(conn: &RustConnection, name: &[u8]) -> Option<Atom> {
    Some(conn.intern_atom(false, name).ok()?.reply().ok()?.atom)
}

/// `None` stands for every case that ends in `Offer::Unknown`.
fn query(selection: Selection) -> Option<Offer> {
    let (conn, screen) = RustConnection::connect(None).ok()?;
    let root = conn.setup().roots.get(screen)?.root;
    let selection_atom = match selection {
        Selection::Clipboard => intern(&conn, b"CLIPBOARD")?,
        Selection::Primary => AtomEnum::PRIMARY.into(),
    };
    let owner = conn
        .get_selection_owner(selection_atom)
        .ok()?
        .reply()
        .ok()?
        .owner;
    if owner == NONE {
        return None;
    }
    let targets = intern(&conn, b"TARGETS")?;
    let property = intern(&conn, PROPERTY)?;
    let text = TEXT_TARGETS
        .iter()
        .map(|name| intern(&conn, name))
        .collect::<Option<Vec<Atom>>>()?;

    let window = conn.generate_id().ok()?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        window,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        COPY_FROM_PARENT,
        &CreateWindowAux::new(),
    )
    .ok()?;
    conn.convert_selection(window, selection_atom, targets, property, CURRENT_TIME)
        .ok()?;
    conn.flush().ok()?;

    if !answered(&conn, window, selection_atom, targets)? {
        return None;
    }
    let reply = conn
        .get_property(true, window, property, AtomEnum::ANY, 0, u32::MAX / 4)
        .ok()?
        .reply()
        .ok()?;
    // A list of targets has the type `ATOM`; some owners use `TARGETS` instead. Anything
    // else, such as an incremental transfer, is not understood here.
    let atom_type: Atom = AtomEnum::ATOM.into();
    if reply.type_ != atom_type && reply.type_ != targets {
        return None;
    }
    let listed: Vec<Atom> = reply.value32()?.collect();
    Some(decide(&listed, &text))
}

/// Waits for the owner's `SelectionNotify`. `Some(true)` if it stored the list of
/// targets, `Some(false)` if it refused, `None` on a connection error or a time-out.
fn answered(conn: &RustConnection, window: Window, selection: Atom, targets: Atom) -> Option<bool> {
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        match conn.poll_for_event().ok()? {
            Some(Event::SelectionNotify(event))
                if event.requestor == window
                    && event.selection == selection
                    && event.target == targets =>
            {
                return Some(event.property != NONE);
            }
            Some(_) => {}
            None => thread::sleep(Duration::from_millis(1)),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: [Atom; 3] = [31, 40, 41];

    #[test]
    fn decide_text_target_listed() {
        assert_eq!(decide(&[300, 31], &TEXT), Offer::Text);
        assert_eq!(decide(&[41], &TEXT), Offer::Text);
    }

    #[test]
    fn decide_no_text_target_listed() {
        assert_eq!(decide(&[300, 301], &TEXT), Offer::NoText);
        assert_eq!(decide(&[], &TEXT), Offer::NoText);
    }

    #[test]
    fn decide_ignores_none() {
        assert_eq!(decide(&[NONE], &[NONE, 31]), Offer::NoText);
    }
}

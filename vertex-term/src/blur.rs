use x11rb::{
    connection::Connection,
    protocol::xproto::*,
    rust_connection::RustConnection,
};

/// Set or remove the KDE blur-behind property on our window.
/// Matches the window by PID so it works regardless of title or XWayland layering.
/// Silently no-ops when not running under X11 / XWayland (e.g. pure Wayland session
/// without DISPLAY set).
pub fn apply(enable: bool) {
    let _ = apply_inner(enable);
}

fn apply_inner(enable: bool) -> anyhow::Result<()> {
    let (conn, screen_num) = RustConnection::connect(None)?;
    let root = conn.setup().roots[screen_num].root;

    let blur_atom   = intern(&conn, b"_KDE_NET_WM_BLUR_BEHIND_REGION")?;
    let client_list = intern(&conn, b"_NET_CLIENT_LIST")?;
    let pid_atom    = intern(&conn, b"_NET_WM_PID")?;

    let prop = conn
        .get_property(false, root, client_list, AtomEnum::WINDOW, 0, u32::MAX)?
        .reply()?;
    let windows: Vec<Window> = prop.value32().into_iter().flatten().collect();

    let our_pid = std::process::id();

    for win in windows {
        let pid_prop = conn
            .get_property(false, win, pid_atom, AtomEnum::CARDINAL, 0, 1)?
            .reply()?;

        let win_pid = pid_prop.value32().and_then(|mut i| i.next()).unwrap_or(0);
        if win_pid != our_pid { continue; }

        if enable {
            // Empty cardinal list = blur the entire window region.
            conn.change_property(
                PropMode::REPLACE,
                win,
                blur_atom,
                AtomEnum::CARDINAL,
                32,
                0,
                &[],
            )?;
        } else {
            conn.delete_property(win, blur_atom)?;
        }
        conn.flush()?;
        return Ok(());
    }
    Ok(())
}

fn intern(conn: &impl Connection, name: &[u8]) -> anyhow::Result<Atom> {
    Ok(conn.intern_atom(false, name)?.reply()?.atom)
}

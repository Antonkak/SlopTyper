use std::thread;
use crossbeam_channel::Sender;
use crossterm::event::{self, Event};

/// Spawns a thread that reads keyboard events from the appropriate source
/// and sends them through the provided channel.
/// 
/// On Unix, when stdin is redirected (piped), reads from /dev/tty.
/// Otherwise reads from stdin (default crossterm behavior).
pub fn spawn_key_event_reader(key_event_tx: Sender<Event>, stdin_piped: bool) {
    thread::spawn(move || {
        #[cfg(unix)]
        {
            if stdin_piped {
                // On Unix with piped stdin, read from /dev/tty
                if let Ok(tty) = std::fs::File::open("/dev/tty") {
                    read_events_from_tty(&tty, key_event_tx);
                    return;
                }
            }
        }
        
        #[cfg(not(unix))]
        {
            let _ = stdin_piped; // Suppress unused warning on non-Unix
        }
        
        // Default: read from stdin (crossterm's default)
        read_events_from_stdin(key_event_tx);
    });
}

#[cfg(unix)]
fn read_events_from_tty(tty: &std::fs::File, key_event_tx: Sender<Event>) {
    use std::os::fd::AsRawFd;
    
    let tty_fd = tty.as_raw_fd();
    
    // Save original stdin fd
    let original_stdin_fd = unsafe { libc::dup(libc::STDIN_FILENO) };
    
    // Redirect stdin to /dev/tty for crossterm
    unsafe {
        libc::dup2(tty_fd, libc::STDIN_FILENO);
    }
    
    // Enable raw mode on the tty
    let _ = crossterm::terminal::enable_raw_mode();
    
    // Read events using crossterm (now reading from /dev/tty)
    loop {
        match event::read() {
            Ok(event) => {
                if key_event_tx.send(event).is_err() {
                    break; // Channel closed
                }
            }
            Err(e) => {
                tracing::error!("Error reading from /dev/tty: {}", e);
                break;
            }
        }
    }
    
    // Restore original stdin
    unsafe {
        libc::dup2(original_stdin_fd, libc::STDIN_FILENO);
        libc::close(original_stdin_fd);
    }
    let _ = crossterm::terminal::disable_raw_mode();
}

fn read_events_from_stdin(key_event_tx: Sender<Event>) {
    loop {
        match event::read() {
            Ok(event) => {
                if key_event_tx.send(event).is_err() {
                    break; // Channel closed
                }
            }
            Err(e) => {
                tracing::error!("Error reading from stdin: {}", e);
                break;
            }
        }
    }
}
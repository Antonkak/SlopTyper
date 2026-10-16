use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use crossbeam_channel::{Sender, Receiver, unbounded, bounded};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::{execute, queue};
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{enable_raw_mode, disable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, Clear, ClearType};
use crossterm::cursor::MoveTo;
use crate::buffer_manager::BufferManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookState {
    Idle,
    Active,
}

pub struct KeyboardHook {
    state: Arc<AtomicBool>,
    buffer_manager: BufferManager,
    command_tx: Sender<HookCommand>,
    command_rx: Receiver<HookCommand>,
    key_event_tx: Sender<Event>,
    key_event_rx: Receiver<Event>,
}

#[derive(Debug, Clone)]
pub enum HookCommand {
    StartIntercept,
    StopIntercept,
    InjectKeystrokes(String),
    Quit,
}

impl KeyboardHook {
    pub fn new(buffer_manager: BufferManager) -> Self {
        let (command_tx, command_rx) = unbounded();
        let (key_event_tx, key_event_rx) = bounded(100);
        
        Self {
            state: Arc::new(AtomicBool::new(false)),
            buffer_manager,
            command_tx,
            command_rx,
            key_event_tx,
            key_event_rx,
        }
    }

    pub fn get_command_sender(&self) -> Sender<HookCommand> {
        self.command_tx.clone()
    }

    pub fn get_command_receiver(&self) -> Receiver<HookCommand> {
        self.command_rx.clone()
    }

    pub fn get_key_event_sender(&self) -> Sender<Event> {
        self.key_event_tx.clone()
    }

    pub fn is_intercept_active(&self) -> bool {
        self.state.load(Ordering::Relaxed)
    }

    pub fn run_tui_loop(&self) -> anyhow::Result<()> {
        let state = self.state.clone();
        let buffer_manager = self.buffer_manager.clone();
        let key_event_rx = self.key_event_rx.clone();
        
        let mut stdout = std::io::stdout();
        enable_raw_mode()?;
        let _ = execute!(stdout, EnterAlternateScreen, Clear(ClearType::All), MoveTo(0, 0));
        
        // Clear any pending input (e.g., stray Enter from shell)
        while event::poll(std::time::Duration::from_millis(10))? {
            let _ = event::read();
        }
        
        tracing::info!("TUI Mode - Press any key to inject code, Esc/Enter to copy and exit");

        let result = (|| -> anyhow::Result<()> {
            loop {
                // Try to receive key event from channel (non-blocking)
                let key_event = match key_event_rx.try_recv() {
                    Ok(event) => Some(event),
                    Err(crossbeam_channel::TryRecvError::Empty) => None,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        tracing::warn!("Key event channel disconnected");
                        break;
                    }
                };

                if let Some(Event::Key(key_event)) = key_event {
                    if key_event.kind == KeyEventKind::Press {
                        match key_event.code {
                            KeyCode::Esc | KeyCode::Enter => {
                                if buffer_manager.has_content() {
                                    let content = buffer_manager.get_all_content();
                                    if let Err(e) = copy_to_clipboard(&content) {
                                        tracing::error!("Failed to copy to clipboard: {}", e);
                                    } else {
                                        tracing::info!("Copied {} characters to clipboard", content.len());
                                    }
                                }
                                break;
                            }
                            _ => {
                                let active = state.load(Ordering::Relaxed);
                                if active {
                                    if !buffer_manager.has_content() {
                                        tracing::warn!("Buffer empty");
                                        break;
                                    }
                                    let batch = buffer_manager.get_next_batch();
                                    if !batch.is_empty() {
                                        tracing::info!("Injecting: {}", batch);
                                        let _ = execute!(stdout, SetForegroundColor(Color::Green));
                                        let _ = queue!(stdout, Print(batch.clone()));
                                        let _ = execute!(stdout, ResetColor);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // No key event available, yield to avoid busy loop
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            Ok(())
        })();
        
        let _ = disable_raw_mode();
        let _ = execute!(stdout, LeaveAlternateScreen, Clear(ClearType::All));
        result
    }

    pub fn start_tui(&self) -> anyhow::Result<()> {
        self.state.store(true, Ordering::Relaxed);
        self.run_tui_loop()
    }

    pub fn process_commands(&mut self) -> bool {
        let triggered = false;
        while let Ok(cmd) = self.command_rx.try_recv() {
            match cmd {
                HookCommand::StartIntercept => {
                    self.state.store(true, Ordering::Relaxed);
                    tracing::info!("TUI mode ACTIVE - press any key to inject code");
                }
                HookCommand::StopIntercept => {
                    self.state.store(false, Ordering::Relaxed);
                    tracing::info!("TUI mode STOPPED");
                }
                HookCommand::InjectKeystrokes(text) => {
                    tracing::info!("Injecting: {}", text);
                    let mut stdout = std::io::stdout();
                    let _ = execute!(stdout, SetForegroundColor(Color::Green));
                    let _ = queue!(stdout, Print(text));
                    let _ = execute!(stdout, ResetColor);
                }
                HookCommand::Quit => {
                    self.state.store(false, Ordering::Relaxed);
                    break;
                }
            }
        }
        triggered
    }

    pub fn activate_intercept(&self) {
        let _ = self.command_tx.send(HookCommand::StartIntercept);
    }

    pub fn deactivate_intercept(&self) {
        let _ = self.command_tx.send(HookCommand::StopIntercept);
    }
}

impl Clone for KeyboardHook {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            buffer_manager: self.buffer_manager.clone(),
            command_tx: self.command_tx.clone(),
            command_rx: self.command_rx.clone(),
            key_event_tx: self.key_event_tx.clone(),
            key_event_rx: self.key_event_rx.clone(),
        }
    }
}

fn copy_to_clipboard(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(text.to_string())?;
    Ok(())
}
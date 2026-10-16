use std::io;
use tokio::io::{AsyncBufReadExt, BufReader};
use crate::buffer_manager::{BufferManager, BufferCommand};

pub async fn spawn_stdin_reader(buffer_manager: BufferManager) -> anyhow::Result<()> {
    let mut reader = BufReader::new(tokio::io::stdin());
    let mut line = String::new();
    let mut first_chunk = true;

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        
        if bytes_read == 0 {
            // EOF reached - stdin closed
            tracing::info!("STDIN EOF reached, keeping buffer available for typing");
            break;
        }

        // For the first chunk, replace the buffer; for subsequent chunks, append
        if first_chunk {
            let _ = buffer_manager.get_command_sender().send(BufferCommand::Load(line.clone()));
            first_chunk = false;
        } else {
            let _ = buffer_manager.get_command_sender().send(BufferCommand::Append(line.clone()));
        }
        buffer_manager.process_commands();
        
        tracing::debug!("Read {} bytes from stdin, buffer size: {}", 
            bytes_read, buffer_manager.remaining_len());
    }

    Ok(())
}

pub fn is_stdin_piped() -> bool {
    !is_terminal::is_terminal(&io::stdin())
}
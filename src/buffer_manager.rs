use std::sync::{Arc, Mutex};
use crossbeam_channel::{Sender, Receiver, unbounded};
use rand::Rng;

#[derive(Debug, Clone)]
pub struct CodeBuffer {
    pub content: String,
    pub position: usize,
}

impl CodeBuffer {
    pub fn new(content: String) -> Self {
        Self { content, position: 0 }
    }

    pub fn remaining(&self) -> &str {
        &self.content[self.position..]
    }

    pub fn is_empty(&self) -> bool {
        // Buffer is empty if content is empty or position is at end (but will wrap)
        self.content.is_empty()
    }

    pub fn take_batch(&mut self, size: usize) -> String {
        let start = self.position;
        let end = (self.position + size).min(self.content.len());
        self.position = end;
        
        // If we've reached the end, wrap around
        if self.position >= self.content.len() {
            self.position = 0;
        }
        
        self.content[start..end].to_string()
    }

    pub fn reset(&mut self) {
        self.position = 0;
    }
}

#[derive(Debug, Clone)]
pub enum BufferCommand {
    Load(String),
    Append(String),
    Clear,
    Reset,
}

#[derive(Debug, Clone)]
pub struct BufferManager {
    buffer: Arc<Mutex<CodeBuffer>>,
    command_tx: Sender<BufferCommand>,
    command_rx: Receiver<BufferCommand>,
    batch_size_range: (usize, usize),
}

impl BufferManager {
    pub fn new(initial_content: String, batch_size_range: (usize, usize)) -> Self {
        let (command_tx, command_rx) = unbounded();
        let buffer = Arc::new(Mutex::new(CodeBuffer::new(initial_content)));
        
        Self {
            buffer,
            command_tx,
            command_rx,
            batch_size_range,
        }
    }

    pub fn get_command_sender(&self) -> Sender<BufferCommand> {
        self.command_tx.clone()
    }

    pub fn get_next_batch(&self) -> String {
        let mut buffer = self.buffer.lock().unwrap();
        if buffer.content.is_empty() {
            return String::new();
        }
        
        let batch_size = rand::thread_rng().gen_range(self.batch_size_range.0..=self.batch_size_range.1);
        buffer.take_batch(batch_size)
    }

    pub fn has_content(&self) -> bool {
        let buffer = self.buffer.lock().unwrap();
        !buffer.is_empty()
    }

    pub fn remaining_len(&self) -> usize {
        let buffer = self.buffer.lock().unwrap();
        if buffer.content.is_empty() {
            0
        } else {
            buffer.content.len() - buffer.position
        }
    }

    pub fn process_commands(&self) {
        while let Ok(cmd) = self.command_rx.try_recv() {
            let mut buffer = self.buffer.lock().unwrap();
            match cmd {
                BufferCommand::Load(content) => {
                    *buffer = CodeBuffer::new(content);
                }
                BufferCommand::Append(content) => {
                    buffer.content.push_str(&content);
                }
                BufferCommand::Clear => {
                    buffer.content.clear();
                    buffer.position = 0;
                }
                BufferCommand::Reset => {
                    buffer.reset();
                }
            }
        }
    }

    pub fn get_buffer_state(&self) -> (usize, usize) {
        let buffer = self.buffer.lock().unwrap();
        (buffer.position, buffer.content.len())
    }

    pub fn get_all_content(&self) -> String {
        let buffer = self.buffer.lock().unwrap();
        buffer.content.clone()
    }
}

pub fn generate_sample_code() -> String {
    let samples = vec![
        r#"async fn process_stream(mut rx: mpsc::Receiver<Token>) -> Result<()> {
    let mut buffer = String::new();
    while let Some(token) = rx.recv().await {
        buffer.push_str(&token.text);
        if buffer.len() >= BATCH_SIZE {
            inject_keystrokes(&buffer).await?;
            buffer.clear();
        }
    }
    Ok(())
}"#,
        r#"fn fibonacci(n: u64) -> u64 {
    match n {
        0 => 0,
        1 => 1,
        _ => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

fn main() {
    for i in 0..20 {
        println!("fib({}) = {}", i, fibonacci(i));
    }
}"#,
        r#"struct NeuralNetwork {
    layers: Vec<Layer>,
    learning_rate: f64,
}

impl NeuralNetwork {
    fn forward(&self, input: &[f64]) -> Vec<f64> {
        let mut activations = input.to_vec();
        for layer in &self.layers {
            activations = layer.forward(&activations);
        }
        activations
    }
    
    fn backward(&mut self, target: &[f64]) {
        // Backpropagation implementation
    }
}"#,
        r#"use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    
    loop {
        let (mut socket, _) = listener.accept().await?;
        tokio::spawn(async move {
            let mut buf = [0; 1024];
            let n = socket.read(&mut buf).await?;
            socket.write_all(&buf[..n]).await?;
        });
    }
}"#,
    ];
    
    samples[rand::thread_rng().gen_range(0..samples.len())].to_string()
}